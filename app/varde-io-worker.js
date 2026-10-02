let wasm_bindgen = (function(exports) {
    let script_src;
    if (typeof document !== 'undefined' && document.currentScript !== null) {
        script_src = new URL(document.currentScript.src, location.href).toString();
    }
    function __wbg_get_imports() {
        const import0 = {
            __proto__: null,
            __wbg___wbindgen_debug_string_4687d8d8c2017d52: function(arg0, arg1) {
                const ret = debugString(getObject(arg1));
                const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
                const len1 = WASM_VECTOR_LEN;
                getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
                getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
            },
            __wbg___wbindgen_is_function_1f9d30630b8b1d3d: function(arg0) {
                const ret = typeof(getObject(arg0)) === 'function';
                return ret;
            },
            __wbg___wbindgen_is_undefined_8865fb403f8fe9d8: function(arg0) {
                const ret = getObject(arg0) === undefined;
                return ret;
            },
            __wbg___wbindgen_rethrow_cb2e88c6b2a16733: function(arg0) {
                throw takeObject(arg0);
            },
            __wbg___wbindgen_string_get_0380ccaa2f57f0d9: function(arg0, arg1) {
                const obj = getObject(arg1);
                const ret = typeof(obj) === 'string' ? obj : undefined;
                var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
                var len1 = WASM_VECTOR_LEN;
                getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
                getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
            },
            __wbg___wbindgen_throw_41e9ee4f547fc59a: function(arg0, arg1) {
                throw new Error(getStringFromWasm0(arg0, arg1));
            },
            __wbg__wbg_cb_unref_dcc1a90847f04c41: function(arg0) {
                getObject(arg0)._wbg_cb_unref();
            },
            __wbg_abort_b46650b76e524767: function(arg0) {
                const ret = getObject(arg0).abort();
                return addHeapObject(ret);
            },
            __wbg_arrayBuffer_0fe6e1300abcf908: function(arg0) {
                const ret = getObject(arg0).arrayBuffer();
                return addHeapObject(ret);
            },
            __wbg_buffer_56ec2905a66f58b9: function(arg0) {
                const ret = getObject(arg0).buffer;
                return addHeapObject(ret);
            },
            __wbg_close_4d4e83804e073061: function(arg0) {
                const ret = getObject(arg0).close();
                return addHeapObject(ret);
            },
            __wbg_close_934c4c1f8dc25c8a: function(arg0) {
                getObject(arg0).close();
            },
            __wbg_concat_1bf80de32bbc2095: function(arg0, arg1) {
                const ret = getObject(arg0).concat(getObject(arg1));
                return addHeapObject(ret);
            },
            __wbg_createSyncAccessHandle_ac771882e87a924d: function(arg0) {
                const ret = getObject(arg0).createSyncAccessHandle();
                return addHeapObject(ret);
            },
            __wbg_createWritable_b127e5f5ceb19a0e: function(arg0) {
                const ret = getObject(arg0).createWritable();
                return addHeapObject(ret);
            },
            __wbg_data_522f7abc70721269: function(arg0) {
                const ret = getObject(arg0).data;
                return addHeapObject(ret);
            },
            __wbg_done_b41a1d26cdb37fb6: function(arg0) {
                const ret = getObject(arg0).done;
                return ret;
            },
            __wbg_error_c9cf3fc2064683a9: function(arg0) {
                console.error(getObject(arg0));
            },
            __wbg_flush_cc7a0c869c969595: function() { return handleError(function (arg0) {
                getObject(arg0).flush();
            }, arguments); },
            __wbg_getDirectoryHandle_32ff6719da8ccf70: function(arg0, arg1, arg2, arg3) {
                const ret = getObject(arg0).getDirectoryHandle(getStringFromWasm0(arg1, arg2), getObject(arg3));
                return addHeapObject(ret);
            },
            __wbg_getDirectory_8257f053b0d3e746: function(arg0) {
                const ret = getObject(arg0).getDirectory();
                return addHeapObject(ret);
            },
            __wbg_getFileHandle_0357a6e30a3f80ba: function(arg0, arg1, arg2, arg3) {
                const ret = getObject(arg0).getFileHandle(getStringFromWasm0(arg1, arg2), getObject(arg3));
                return addHeapObject(ret);
            },
            __wbg_getFile_75187c3eb15ebeee: function(arg0) {
                const ret = getObject(arg0).getFile();
                return addHeapObject(ret);
            },
            __wbg_getSize_b3c9adeccff7e656: function() { return handleError(function (arg0) {
                const ret = getObject(arg0).getSize();
                return ret;
            }, arguments); },
            __wbg_get_unchecked_288889d017702237: function(arg0, arg1) {
                const ret = getObject(arg0)[arg1 >>> 0];
                return addHeapObject(ret);
            },
            __wbg_instanceof_ArrayBuffer_a99f175873e5d9b8: function(arg0) {
                let result;
                try {
                    result = getObject(arg0) instanceof ArrayBuffer;
                } catch (_) {
                    result = false;
                }
                const ret = result;
                return ret;
            },
            __wbg_instanceof_DomException_550db3f9465a933f: function(arg0) {
                let result;
                try {
                    result = getObject(arg0) instanceof DOMException;
                } catch (_) {
                    result = false;
                }
                const ret = result;
                return ret;
            },
            __wbg_instanceof_Error_80a725f81f2e102d: function(arg0) {
                let result;
                try {
                    result = getObject(arg0) instanceof Error;
                } catch (_) {
                    result = false;
                }
                const ret = result;
                return ret;
            },
            __wbg_instanceof_FileSystemFileHandle_f5198fa86c4096bc: function(arg0) {
                let result;
                try {
                    result = getObject(arg0) instanceof FileSystemFileHandle;
                } catch (_) {
                    result = false;
                }
                const ret = result;
                return ret;
            },
            __wbg_instanceof_File_b5643aaf31866db3: function(arg0) {
                let result;
                try {
                    result = getObject(arg0) instanceof File;
                } catch (_) {
                    result = false;
                }
                const ret = result;
                return ret;
            },
            __wbg_isArray_e15a2ff68ffdbef2: function(arg0) {
                const ret = Array.isArray(getObject(arg0));
                return ret;
            },
            __wbg_keys_b33fd056a9da29ee: function(arg0) {
                const ret = getObject(arg0).keys();
                return addHeapObject(ret);
            },
            __wbg_lastModified_6c50da7084f8ec7e: function(arg0) {
                const ret = getObject(arg0).lastModified;
                return ret;
            },
            __wbg_length_7f3c00c40364105e: function(arg0) {
                const ret = getObject(arg0).length;
                return ret;
            },
            __wbg_length_d4bdea10311bd9cf: function(arg0) {
                const ret = getObject(arg0).length;
                return ret;
            },
            __wbg_message_5f8387f0c32b90a7: function(arg0) {
                const ret = getObject(arg0).message;
                return addHeapObject(ret);
            },
            __wbg_message_d988ea596c5f5c9a: function(arg0, arg1) {
                const ret = getObject(arg1).message;
                const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
                const len1 = WASM_VECTOR_LEN;
                getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
                getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
            },
            __wbg_name_30c2cf5d3e6226e8: function(arg0, arg1) {
                const ret = getObject(arg1).name;
                const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
                const len1 = WASM_VECTOR_LEN;
                getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
                getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
            },
            __wbg_name_e2eac7cdfa054f65: function(arg0) {
                const ret = getObject(arg0).name;
                return addHeapObject(ret);
            },
            __wbg_navigator_c9bce48d4b578c9c: function(arg0) {
                const ret = getObject(arg0).navigator;
                return addHeapObject(ret);
            },
            __wbg_new_1dbf7428bba60a42: function(arg0) {
                const ret = new Uint8Array(getObject(arg0));
                return addHeapObject(ret);
            },
            __wbg_new_617a8cdb8bb1130e: function() {
                const ret = new Object();
                return addHeapObject(ret);
            },
            __wbg_new_ee2291f50781bf1d: function() {
                const ret = new Array();
                return addHeapObject(ret);
            },
            __wbg_new_from_slice_9a868026ffa4208a: function(arg0, arg1) {
                const ret = new Uint8Array(getArrayU8FromWasm0(arg0, arg1));
                return addHeapObject(ret);
            },
            __wbg_next_762aca09b0915bfc: function() { return handleError(function (arg0) {
                const ret = getObject(arg0).next();
                return addHeapObject(ret);
            }, arguments); },
            __wbg_now_aa4ccb83129e9e55: function() {
                const ret = Date.now();
                return ret;
            },
            __wbg_postMessage_170a79621f69b186: function() { return handleError(function (arg0, arg1, arg2) {
                getObject(arg0).postMessage(getObject(arg1), getObject(arg2));
            }, arguments); },
            __wbg_postMessage_7dd4fec24fe9919c: function() { return handleError(function (arg0, arg1) {
                getObject(arg0).postMessage(getObject(arg1));
            }, arguments); },
            __wbg_prototypesetcall_bc27214492979395: function(arg0, arg1, arg2) {
                Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), getObject(arg2));
            },
            __wbg_push_2baf45db356cf468: function(arg0, arg1) {
                const ret = getObject(arg0).push(getObject(arg1));
                return ret;
            },
            __wbg_queueMicrotask_9833f9a49df95a49: function(arg0) {
                const ret = getObject(arg0).queueMicrotask;
                return addHeapObject(ret);
            },
            __wbg_queueMicrotask_a72f977e97f23c5f: function(arg0) {
                queueMicrotask(getObject(arg0));
            },
            __wbg_random_5a4cafd2f02395ff: function() {
                const ret = Math.random();
                return ret;
            },
            __wbg_read_fcd52999af006e79: function() { return handleError(function (arg0, arg1, arg2, arg3) {
                const ret = getObject(arg0).read(getArrayU8FromWasm0(arg1, arg2), getObject(arg3));
                return ret;
            }, arguments); },
            __wbg_removeEntry_4d962be4429d4c17: function(arg0, arg1, arg2) {
                const ret = getObject(arg0).removeEntry(getStringFromWasm0(arg1, arg2));
                return addHeapObject(ret);
            },
            __wbg_resolve_0076e10020304ede: function(arg0) {
                const ret = Promise.resolve(getObject(arg0));
                return addHeapObject(ret);
            },
            __wbg_set_at_a00f11a442de8f74: function(arg0, arg1) {
                getObject(arg0).at = arg1;
            },
            __wbg_set_create_4f5fb19abc6eb52a: function(arg0, arg1) {
                getObject(arg0).create = arg1 !== 0;
            },
            __wbg_set_create_55977c3183b4be6e: function(arg0, arg1) {
                getObject(arg0).create = arg1 !== 0;
            },
            __wbg_set_onmessage_f75882137d0d035f: function(arg0, arg1) {
                getObject(arg0).onmessage = getObject(arg1);
            },
            __wbg_size_8f1c0b1a1fbb3810: function(arg0) {
                const ret = getObject(arg0).size;
                return ret;
            },
            __wbg_slice_afd8274aed4149c0: function() { return handleError(function (arg0, arg1, arg2) {
                const ret = getObject(arg0).slice(arg1, arg2);
                return addHeapObject(ret);
            }, arguments); },
            __wbg_static_accessor_GLOBAL_266715b9d96ba635: function() {
                const ret = typeof global === 'undefined' ? null : global;
                return isLikeNone(ret) ? 0 : addHeapObject(ret);
            },
            __wbg_static_accessor_GLOBAL_THIS_10fb7dc1ae063179: function() {
                const ret = typeof globalThis === 'undefined' ? null : globalThis;
                return isLikeNone(ret) ? 0 : addHeapObject(ret);
            },
            __wbg_static_accessor_SELF_0b583911f537483a: function() {
                const ret = typeof self === 'undefined' ? null : self;
                return isLikeNone(ret) ? 0 : addHeapObject(ret);
            },
            __wbg_static_accessor_WINDOW_d7f903d1508cbdc4: function() {
                const ret = typeof window === 'undefined' ? null : window;
                return isLikeNone(ret) ? 0 : addHeapObject(ret);
            },
            __wbg_storage_46a39f4b6d8508a3: function(arg0) {
                const ret = getObject(arg0).storage;
                return addHeapObject(ret);
            },
            __wbg_then_c949d5a25a4e78f8: function(arg0, arg1, arg2) {
                const ret = getObject(arg0).then(getObject(arg1), getObject(arg2));
                return addHeapObject(ret);
            },
            __wbg_then_e71170d78fcf8954: function(arg0, arg1) {
                const ret = getObject(arg0).then(getObject(arg1));
                return addHeapObject(ret);
            },
            __wbg_truncate_a598d549f268301b: function() { return handleError(function (arg0, arg1) {
                getObject(arg0).truncate(arg1);
            }, arguments); },
            __wbg_value_f3c585ee8f5ba40c: function(arg0) {
                const ret = getObject(arg0).value;
                return addHeapObject(ret);
            },
            __wbg_write_7e3b24737a66970d: function() { return handleError(function (arg0, arg1, arg2) {
                const ret = getObject(arg0).write(getArrayU8FromWasm0(arg1, arg2));
                return addHeapObject(ret);
            }, arguments); },
            __wbg_write_9e2910049de41b0a: function() { return handleError(function (arg0, arg1, arg2, arg3) {
                const ret = getObject(arg0).write(getArrayU8FromWasm0(arg1, arg2), getObject(arg3));
                return ret;
            }, arguments); },
            __wbindgen_generic_0000000000000001: function(arg0, arg1) {
                // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 17, ret: Result(Unit), inner_ret: Some(Result(Unit)) }, mutable: true }) -> Externref`.
                const ret = makeMutClosure(arg0, arg1, __wasm_bindgen_func_elem_222);
                return addHeapObject(ret);
            },
            __wbindgen_generic_0000000000000002: function(arg0, arg1) {
                // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [NamedExternref("MessageEvent")], shim_idx: 1, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
                const ret = makeMutClosure(arg0, arg1, __wasm_bindgen_func_elem_2956);
                return addHeapObject(ret);
            },
            __wbindgen_generic_0000000000000003: function(arg0, arg1) {
                // Cast intrinsic for `Ref(String) -> Externref`.
                const ret = getStringFromWasm0(arg0, arg1);
                return addHeapObject(ret);
            },
            __wbindgen_object_clone_ref: function(arg0) {
                const ret = getObject(arg0);
                return addHeapObject(ret);
            },
            __wbindgen_object_drop_ref: function(arg0) {
                takeObject(arg0);
            },
        };
        return {
            __proto__: null,
            "./varde-io-worker_bg.js": import0,
        };
    }

    function __wasm_bindgen_func_elem_2956(arg0, arg1, arg2) {
        wasm.__wasm_bindgen_func_elem_2956(arg0, arg1, addHeapObject(arg2));
    }

    function __wasm_bindgen_func_elem_222(arg0, arg1, arg2) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.__wasm_bindgen_func_elem_222(retptr, arg0, arg1, addHeapObject(arg2));
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }

    function addHeapObject(obj) {
        if (heap_next === heap.length) heap.push(heap.length + 1);
        const idx = heap_next;
        heap_next = heap[idx];

        heap[idx] = obj;
        return idx;
    }

    const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
        ? { register: () => {}, unregister: () => {} }
        : new FinalizationRegistry(state => wasm.__wbindgen_export4(state.a, state.b));

    function debugString(val) {
        // primitive types
        const type = typeof val;
        if (type == 'number' || type == 'boolean' || val == null) {
            return  `${val}`;
        }
        if (type == 'string') {
            return `"${val}"`;
        }
        if (type == 'symbol') {
            const description = val.description;
            if (description == null) {
                return 'Symbol';
            } else {
                return `Symbol(${description})`;
            }
        }
        if (type == 'function') {
            const name = val.name;
            if (typeof name == 'string' && name.length > 0) {
                return `Function(${name})`;
            } else {
                return 'Function';
            }
        }
        // objects
        if (Array.isArray(val)) {
            const length = val.length;
            let debug = '[';
            if (length > 0) {
                debug += debugString(val[0]);
            }
            for(let i = 1; i < length; i++) {
                debug += ', ' + debugString(val[i]);
            }
            debug += ']';
            return debug;
        }
        // Test for built-in
        const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
        let className;
        if (builtInMatches && builtInMatches.length > 1) {
            className = builtInMatches[1];
        } else {
            // Failed to match the standard '[object ClassName]'
            return toString.call(val);
        }
        if (className == 'Object') {
            // we're a user defined class or Object
            // JSON.stringify avoids problems with cycles, and is generally much
            // easier than looping through ownProperties of `val`.
            try {
                return 'Object(' + JSON.stringify(val) + ')';
            } catch (_) {
                return 'Object';
            }
        }
        // errors
        if (val instanceof Error) {
            return `${val.name}: ${val.message}\n${val.stack}`;
        }
        // TODO we could test for more things here, like `Set`s and `Map`s.
        return className;
    }

    function dropObject(idx) {
        if (idx < 1028) return;
        heap[idx] = heap_next;
        heap_next = idx;
    }

    function getArrayU8FromWasm0(ptr, len) {
        ptr = ptr >>> 0;
        return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
    }

    let cachedDataViewMemory0 = null;
    function getDataViewMemory0() {
        if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
            cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
        }
        return cachedDataViewMemory0;
    }

    function getStringFromWasm0(ptr, len) {
        return decodeText(ptr >>> 0, len);
    }

    let cachedUint8ArrayMemory0 = null;
    function getUint8ArrayMemory0() {
        if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
            cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
        }
        return cachedUint8ArrayMemory0;
    }

    function getObject(idx) { return heap[idx]; }

    function handleError(f, args) {
        try {
            return f.apply(this, args);
        } catch (e) {
            wasm.__wbindgen_export3(addHeapObject(e));
        }
    }

    let heap = new Array(1024).fill(undefined);
    heap.push(undefined, null, true, false);

    let heap_next = heap.length;

    function isLikeNone(x) {
        return x === undefined || x === null;
    }

    function makeMutClosure(arg0, arg1, f) {
        const state = { a: arg0, b: arg1, cnt: 1 };
        const real = (...args) => {

            // First up with a closure we increment the internal reference
            // count. This ensures that the Rust closure environment won't
            // be deallocated while we're invoking it.
            state.cnt++;
            const a = state.a;
            state.a = 0;
            try {
                return f(a, state.b, ...args);
            } finally {
                state.a = a;
                real._wbg_cb_unref();
            }
        };
        real._wbg_cb_unref = () => {
            if (--state.cnt === 0) {
                wasm.__wbindgen_export4(state.a, state.b);
                state.a = 0;
                CLOSURE_DTORS.unregister(state);
            }
        };
        CLOSURE_DTORS.register(real, state, state);
        return real;
    }

    function passStringToWasm0(arg, malloc, realloc) {
        if (realloc === undefined) {
            const buf = cachedTextEncoder.encode(arg);
            const ptr = malloc(buf.length, 1) >>> 0;
            getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
            WASM_VECTOR_LEN = buf.length;
            return ptr;
        }

        let len = arg.length;
        let ptr = malloc(len, 1) >>> 0;

        const mem = getUint8ArrayMemory0();

        let offset = 0;

        for (; offset < len; offset++) {
            const code = arg.charCodeAt(offset);
            if (code > 0x7F) break;
            mem[ptr + offset] = code;
        }
        if (offset !== len) {
            if (offset !== 0) {
                arg = arg.slice(offset);
            }
            ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
            const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
            const ret = cachedTextEncoder.encodeInto(arg, view);

            offset += ret.written;
            ptr = realloc(ptr, len, offset, 1) >>> 0;
        }

        WASM_VECTOR_LEN = offset;
        return ptr;
    }

    function takeObject(idx) {
        const ret = getObject(idx);
        dropObject(idx);
        return ret;
    }

    let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
    cachedTextDecoder.decode();
    function decodeText(ptr, len) {
        return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
    }

    const cachedTextEncoder = new TextEncoder();

    if (!('encodeInto' in cachedTextEncoder)) {
        cachedTextEncoder.encodeInto = function (arg, view) {
            const buf = cachedTextEncoder.encode(arg);
            view.set(buf);
            return {
                read: arg.length,
                written: buf.length
            };
        };
    }

    let WASM_VECTOR_LEN = 0;

    let wasmModule, wasmInstance, wasm;
    function __wbg_finalize_init(instance, module) {
        wasmInstance = instance;
        wasm = instance.exports;
        wasmModule = module;
        cachedDataViewMemory0 = null;
        cachedUint8ArrayMemory0 = null;
        wasm.__wbindgen_start();
        return wasm;
    }

    async function __wbg_load(module, imports) {
        if (typeof Response === 'function' && module instanceof Response) {
            if (!module.ok) {
                throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
            }

            if (typeof WebAssembly.instantiateStreaming === 'function') {
                try {
                    return await WebAssembly.instantiateStreaming(module, imports);
                } catch (e) {
                    const validResponse = expectedResponseType(module.type);

                    if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                        console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                    } else { throw e; }
                }
            }

            const bytes = await module.arrayBuffer();
            return await WebAssembly.instantiate(bytes, imports);
        } else {
            const instance = await WebAssembly.instantiate(module, imports);

            if (instance instanceof WebAssembly.Instance) {
                return { instance, module };
            } else {
                return instance;
            }
        }

        function expectedResponseType(type) {
            switch (type) {
                case 'basic': case 'cors': case 'default': return true;
            }
            return false;
        }
    }

    function initSync(module) {
        if (wasm !== undefined) return wasm;


        if (module !== undefined) {
            if (Object.getPrototypeOf(module) === Object.prototype) {
                ({module} = module)
            } else {
                console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
            }
        }

        const imports = __wbg_get_imports();
        if (!(module instanceof WebAssembly.Module)) {
            module = new WebAssembly.Module(module);
        }
        const instance = new WebAssembly.Instance(module, imports);
        return __wbg_finalize_init(instance, module);
    }

    async function __wbg_init(module_or_path) {
        if (wasm !== undefined) return wasm;


        if (module_or_path !== undefined) {
            if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
                ({module_or_path} = module_or_path)
            } else {
                console.warn('using deprecated parameters for the initialization function; pass a single object instead')
            }
        }

        if (module_or_path === undefined && script_src !== undefined) {
            module_or_path = script_src.replace(/\.js$/, "_bg.wasm");
        }
        const imports = __wbg_get_imports();

        if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
            module_or_path = fetch(module_or_path);
        }

        const { instance, module } = await __wbg_load(await module_or_path, imports);

        return __wbg_finalize_init(instance, module);
    }

    return Object.assign(__wbg_init, { initSync }, exports);
})({ __proto__: null });
