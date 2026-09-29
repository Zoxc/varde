use super::*;

#[test]
fn sizes_from_js_are_whole_and_exact() {
    assert_eq!(size(0.0).unwrap(), 0);
    assert_eq!(size(4096.0).unwrap(), 4096);
    assert_eq!(size(MAX_SAFE_INTEGER as f64).unwrap(), MAX_SAFE_INTEGER);
    for bad in [
        -1.0,
        0.5,
        f64::NAN,
        f64::INFINITY,
        -0.0 - 1e-300,
        (MAX_SAFE_INTEGER + 1) as f64,
        f64::MAX,
    ] {
        let error = size(bad).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{bad}");
    }
}

#[test]
fn numbers_to_js_are_exact() {
    assert_eq!(number(0).unwrap(), 0.0);
    assert_eq!(number(MAX_SAFE_INTEGER).unwrap(), MAX_SAFE_INTEGER as f64);
    for bad in [MAX_SAFE_INTEGER + 1, u64::MAX] {
        let error = number(bad).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge, "{bad}");
    }
}

#[test]
fn offsets_to_js_are_checked() {
    assert_eq!(offset(0, 0).unwrap(), 0.0);
    assert_eq!(offset(10, 5).unwrap(), 15.0);
    assert_eq!(
        offset(MAX_SAFE_INTEGER, 0).unwrap(),
        MAX_SAFE_INTEGER as f64
    );
    for (at, n) in [(MAX_SAFE_INTEGER, 1), (u64::MAX, 1), (u64::MAX, 0)] {
        let error = offset(at, n).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
    }
}

#[test]
fn lengths_from_js_are_bounded() {
    let too_long = |_| io::Error::from(io::ErrorKind::FileTooLarge);
    assert_eq!(len(0.0, 10, too_long).unwrap(), 0);
    assert_eq!(len(10.0, 10, too_long).unwrap(), 10);
    assert_eq!(
        len(11.0, 10, too_long).unwrap_err().kind(),
        io::ErrorKind::FileTooLarge
    );
    for bad in [-1.0, 0.5, f64::NAN, f64::INFINITY] {
        let error = len(bad, 10, too_long).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{bad}");
    }
}

#[test]
fn times_from_js_are_whole_seconds() {
    assert_eq!(seconds(1_700_000_000_999.0), Some(1_700_000_000));
    assert_eq!(seconds(0.0), Some(0));
    assert_eq!(seconds(-1.0), Some(-1));
    assert_eq!(seconds(f64::NAN), None);
    assert_eq!(seconds(f64::INFINITY), None);
    assert_eq!(seconds(f64::MAX), None);
    assert_eq!(seconds(-f64::MAX), None);
}

#[test]
fn exceptions_map_to_io_errors() {
    assert_eq!(error_kind("NotFoundError"), io::ErrorKind::NotFound);
    assert_eq!(
        error_kind("NoModificationAllowedError"),
        io::ErrorKind::ResourceBusy
    );
    assert_eq!(error_kind("QuotaExceededError"), io::ErrorKind::StorageFull);
    assert_eq!(error_kind("SomethingElse"), io::ErrorKind::Other);
}

#[test]
fn a_file_is_read_only_up_to_what_a_message_carries() {
    assert_eq!(file_len(0.0).unwrap(), 0);
    assert_eq!(file_len(1234.0).unwrap(), 1234);
    assert_eq!(
        file_len(MAX_MESSAGE_BYTES as f64).unwrap(),
        MAX_MESSAGE_BYTES
    );
    for size in [
        MAX_MESSAGE_BYTES as f64 + 1.0,
        1e300,
        f64::INFINITY,
        f64::NAN,
        -1.0,
        0.5,
    ] {
        assert!(file_len(size).is_err(), "{size}");
    }
}
