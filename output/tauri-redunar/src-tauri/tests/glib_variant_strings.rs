use gtk::glib::variant::ToVariant;

// Exercise the GLib instance actually used by GTK3. Optimized execution is
// essential: the immutable variadic out-argument caused null dereferences
// after optimization, even when an unoptimized run appeared to work.
#[test]
fn string_variant_iterator_reads_both_ends_and_preserves_borrows() {
    let value = ["", "first", "Grüße", "last"].to_variant();
    let mut iter = value.array_iter_str().unwrap();
    assert_eq!(iter.len(), 4);
    let first = iter.next().unwrap();
    assert_eq!(first, "");
    assert_eq!(iter.next_back(), Some("last"));
    assert_eq!(iter.next(), Some("first"));
    assert_eq!(iter.next_back(), Some("Grüße"));
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
    assert_eq!(first, "");
}

#[test]
fn string_variant_iterator_supports_skips_last_and_collection() {
    let value = ["zero", "one", "two", "three", "four", "five"].to_variant();
    let mut iter = value.array_iter_str().unwrap();
    assert_eq!(iter.nth(1), Some("one"));
    assert_eq!(iter.nth_back(1), Some("four"));
    assert_eq!(iter.last(), Some("three"));
    assert_eq!(value.array_iter_str().unwrap().last(), Some("five"));
    assert_eq!(
        value.array_iter_str().unwrap().rev().collect::<Vec<_>>(),
        ["five", "four", "three", "two", "one", "zero"]
    );
}

#[test]
fn empty_and_out_of_range_string_variant_iteration_is_fused() {
    let empty = Vec::<String>::new().to_variant();
    let mut iter = empty.array_iter_str().unwrap();
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
    assert_eq!(iter.last(), None);
    let value = ["only"].to_variant();
    let mut iter = value.array_iter_str().unwrap();
    assert_eq!(iter.nth(5), None);
    assert_eq!(iter.next_back(), None);
    let mut iter = value.array_iter_str().unwrap();
    assert_eq!(iter.nth_back(5), None);
    assert_eq!(iter.next(), None);
}
