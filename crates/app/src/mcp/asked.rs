//! The MCP queries a [`Doc`] answers from its lanes: asking regen,
//! the solve lane and the viewport for what a query waits on, and taking
//! up their answers for the calls waiting ([`super::waiting`]).

use iced::futures::channel::oneshot;
use varde_document::{FeatureId, Generation};
use varde_mcp::tools::{Call, ToolReply};
use varde_regen::{Answer, Response};
use varde_sketch::Analysis;
use varde_view::PreviewRequest;

use super::query::{self, Asked};
use super::waiting::{Caller, DRAW_FAILED, Drawn, Ended, MOVED_ON, NO_SOLVER, Wait, Waiter};
use crate::doc::Doc;

impl Doc {
    /// Answers the query `call`, from `from`, from the design as it is:
    /// `body_topology` goes on to wait for its edge facts, `measure` for
    /// regen's measure, `sketch` for its analysis.
    pub(super) fn mcp_query(&mut self, from: Caller, call: Call) {
        match &call {
            Call::BodyTopology(args) => match query::body_topology(self, args) {
                Ok(asked) => self.ask_topology(from, call, asked, false),
                Err(reply) => self.mcp.answer(from, call, reply),
            },
            Call::Sketch(args) => match query::sketch(self, args) {
                Ok(read) => self.ask_analysis(from, call, read),
                Err(reply) => self.mcp.answer(from, call, reply),
            },
            Call::Measure(args) => match query::measure(self, args) {
                Ok((inspect, held)) => self.ask_measure(from, call, inspect, held),
                Err(reply) => self.mcp.answer(from, call, reply),
            },
            Call::Render(args) => match query::render(self, args) {
                Ok(shot) => self.ask_render(from, call, shot),
                Err(reply) => self.mcp.answer(from, call, reply),
            },
            _ => {
                let reply = answer_now(self, &call);
                self.mcp.answer(from, call, reply);
            }
        }
    }

    /// Asks regen for the edge facts `call` waits on, `asked`, and puts
    /// it waiting; `again` if it's the second time.
    fn ask_topology(&mut self, from: Caller, call: Call, asked: Asked, again: bool) {
        let query = varde_regen::Query::Topology(asked.body);
        self.ask_regen(from, call, query, |sent| {
            (sent == (asked.generation, asked.until)).then_some(Wait::Topology { asked, again })
        });
    }

    /// Asks regen to measure `inspect` on the committed model for the
    /// `measure` call `call` from `from`, and puts it waiting
    /// (`Wait::Measure`).
    fn ask_measure(
        &mut self,
        from: Caller,
        call: Call,
        inspect: varde_regen::Inspect,
        held: query::Held,
    ) {
        let query = varde_regen::Query::Measure(Box::new(inspect));
        self.ask_regen(from, call, query, |(generation, until)| {
            Some(Wait::Measure {
                generation,
                until,
                held,
            })
        });
    }

    /// Asks the viewport to draw `shot` for the `render` call `call` from
    /// `from` on its next frame, and puts it waiting (`Wait::Render`).
    fn ask_render(&mut self, from: Caller, call: Call, shot: query::Shot) {
        let tag = self.mcp.next_tag();
        let (send, answer) = oneshot::channel();
        let request = PreviewRequest::render(
            shot.mesh,
            shot.opacity,
            shot.tints,
            shot.shot,
            shot.selected_faces,
            shot.highlights,
            move |image| {
                // Unless the call ended meanwhile.
                let _ = send.send(image);
            },
        );
        self.mcp.render_answers.push((tag, answer));
        self.mcp.add(Waiter {
            tag,
            from,
            call,
            waits: Wait::Render {
                drawn: Drawn(request),
                data: shot.data,
            },
        });
        self.mcp.refresh_renders();
    }

    /// The picture of the `render` call under `tag` came back as `image`,
    /// or `None` if it couldn't be drawn: its PNG is asked of the IO lane
    /// (`Wait::Encoding`), or the call fails. One for no call waiting (it
    /// ended meanwhile) is dropped.
    pub(crate) fn mcp_rendered(
        &mut self,
        io: &mut crate::io::Io,
        tag: u64,
        image: Option<varde_io::thumbnail::Image>,
    ) {
        let Some(mut waiter) = self.mcp.take(tag) else {
            return;
        };
        let Wait::Render { data, .. } = waiter.waits else {
            self.mcp.add(waiter);
            return;
        };
        let Some(image) = image else {
            log::error!("Couldn't draw the picture an MCP client asked for");
            let reply = ToolReply::error(DRAW_FAILED);
            self.mcp.answer(waiter.from, waiter.call, reply);
            return self.mcp.refresh_renders();
        };
        io.send(varde_io::Request::EncodePng {
            owner: self.id.number(),
            tag,
            image,
        });
        waiter.waits = Wait::Encoding { data };
        self.mcp.add(waiter);
        self.mcp.refresh_renders();
    }

    /// The PNG of the `render` call under `tag` came back from the IO
    /// lane, or why it didn't: the call is answered with it as image
    /// content, unless it's past [`ToolReply::IMAGE_BYTES`]. One for no
    /// call waiting (it ended meanwhile) is dropped.
    pub(crate) fn mcp_encoded(&mut self, tag: u64, result: Result<Vec<u8>, String>) {
        let Some(waiter) = self.mcp.take(tag) else {
            return;
        };
        let Wait::Encoding { data } = waiter.waits else {
            return self.mcp.add(waiter);
        };
        let reply = match result {
            Ok(png) if png.len() <= ToolReply::IMAGE_BYTES => ToolReply::data(data).with_png(png),
            Ok(png) => ToolReply::error(format!(
                "the picture is {} KiB as a PNG, past the bound of {} KiB: ask for a \
                 smaller size",
                png.len() / 1024,
                ToolReply::IMAGE_BYTES / 1024
            )),
            Err(error) => {
                log::error!("Couldn't encode the picture an MCP client asked for: {error}");
                ToolReply::error(format!("Varde couldn't encode the picture: {error}"))
            }
        };
        self.mcp.answer(waiter.from, waiter.call, reply);
    }

    /// Asks regen `query` of the committed model for `call` from `from`
    /// ([`Request::Query`], queued in order beside exports: it neither
    /// replaces the user's measure riding on the regeneration nor is
    /// replaced by it), and puts it waiting as `waits` says of the
    /// generation and `until` sent. Without a committed model, or one
    /// other than `waits` wants, the call is answered saying so.
    ///
    /// [`Request::Query`]: varde_regen::Request::Query
    fn ask_regen(
        &mut self,
        from: Caller,
        call: Call,
        query: varde_regen::Query,
        waits: impl FnOnce((Generation, Option<FeatureId>)) -> Option<Wait>,
    ) {
        let tag = self.mcp.next_tag();
        match self.feed.request_query(tag, query).and_then(waits) {
            Some(waits) => self.mcp.add(Waiter {
                tag,
                from,
                call,
                waits,
            }),
            None => self
                .mcp
                .answer(from, call, ToolReply::error(query::REBUILDING)),
        }
    }

    /// Asks the solve lane to analyse the sketch `read` reads, for the
    /// `sketch` call `call` from `from`, and puts it waiting
    /// (`Wait::Analysis`): queued apart from the user's analysis, it
    /// neither replaces it nor is replaced by it. Without a lane yet (it
    /// starts with the design), the call fails saying so.
    fn ask_analysis(&mut self, from: Caller, call: Call, read: query::Read) {
        let Some(solver) = &mut self.solver else {
            let reply = ToolReply::error(NO_SOLVER);
            return self.mcp.answer(from, call, reply);
        };
        let tag = self.mcp.next_tag();
        let document = self.editor.document();
        solver.send(varde_solve::Request::Analyse {
            query: Some(tag),
            revision: self.editor.revision(),
            sketch: read.sketch,
            units: document.units(),
            params: document.params_shared(),
        });
        self.mcp.add(Waiter {
            tag,
            from,
            call,
            waits: Wait::Analysis { data: read.data },
        });
    }

    /// Ends the `sketch` calls waiting on the solve lane's analysis: the
    /// lane was replaced, and their requests with it.
    pub(crate) fn mcp_end_analyses(&mut self) {
        let analysing = |waiter: &Waiter| matches!(waiter.waits, Wait::Analysis { .. });
        (self.mcp).end_where(analysing, Ended::SolverRestarted, |_| true);
    }

    /// Takes the solve lane's answer to the client's analysis `id`, or
    /// why it failed: the `sketch` call waiting under that tag is
    /// answered with its degrees of freedom, or fails saying why. An
    /// answer for no call waiting (one ended meanwhile) is dropped.
    pub(crate) fn sketch_analysed(&mut self, id: u64, analysis: Result<&Analysis, String>) {
        let Some(waiter) = self.mcp.take(id) else {
            return;
        };
        let Waiter {
            from, call, waits, ..
        } = waiter;
        let reply = match (waits, analysis) {
            (Wait::Analysis { data }, Ok(analysis)) => query::analysed(data, analysis),
            (Wait::Analysis { .. }, Err(error)) => {
                log::error!("The solver failed to analyse a sketch for a client: {error}");
                ToolReply::error(format!("the solver failed to analyse the sketch: {error}"))
            }
            _ => ToolReply::error("internal error: only sketch waits on the solver's analysis"),
        };
        self.mcp.answer(from, call, reply);
    }

    /// Takes regen's answer to a [`Request::Query`] of a call waiting,
    /// by its tag, and answers the call: an answer for no call waiting
    /// (one ended meanwhile) is dropped, one of another generation,
    /// `until` or body than asked is an internal error.
    ///
    /// `body_topology` is answered from the committed model if it's
    /// still the one asked about; if it has moved on, the facts are
    /// asked for once more, of the model now, and past that the call is
    /// answered saying so. `measure` is answered with what was
    /// measured, saying of which model.
    ///
    /// [`Request::Query`]: varde_regen::Request::Query
    pub(crate) fn query_answered(&mut self, response: Response) {
        let Response::Answered {
            tag,
            generation,
            until,
            answer,
        } = response
        else {
            return;
        };
        let Some(waiter) = self.mcp.take(tag) else {
            return;
        };
        let Waiter {
            from, call, waits, ..
        } = waiter;
        const OTHER: &str = "internal error: regen answered another question than asked";
        let reply = match (&call, waits, answer) {
            (
                Call::BodyTopology(args),
                Wait::Topology { asked, again },
                Answer::Topology { body, result },
            ) => {
                let reply =
                    if (body, generation, until) == (asked.body, asked.generation, asked.until) {
                        let facts = result.as_deref().map_err(String::as_str);
                        query::listed(self, args, asked, facts)
                    } else {
                        Some(ToolReply::error(OTHER))
                    };
                match reply {
                    Some(reply) => reply,
                    None if !again => match query::body_topology(self, args) {
                        Ok(asked) => return self.ask_topology(from, call, asked, true),
                        Err(reply) => reply,
                    },
                    None => ToolReply::error(MOVED_ON),
                }
            }
            (
                Call::Measure(args),
                Wait::Measure {
                    generation: g,
                    until: u,
                    held,
                },
                Answer::Measured(result),
            ) if (g, u) == (generation, until) => {
                let inspected = result.as_deref().map_err(String::as_str);
                query::measured(self, args, (generation, until), held, inspected)
            }
            (Call::Measure(_), Wait::Measure { .. }, Answer::Measured(_)) => {
                ToolReply::error(OTHER)
            }
            _ => ToolReply::error(
                "internal error: only body_topology and measure wait on regen's answers",
            ),
        };
        self.mcp.answer(from, call, reply);
    }
}

/// The answer to `call` from `doc`, the open design, for the queries
/// answered at once; the others wait ([`Doc::mcp_query`]).
pub(super) fn answer_now(doc: &Doc, call: &Call) -> ToolReply {
    match call {
        Call::DesignSummary(page) => query::design_summary(doc, *page),
        Call::Feature { name, page } => query::feature(doc, name, *page),
        _ => ToolReply::error("internal error: not a query answered at once"),
    }
}
