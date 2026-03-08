use bench_obl_generic_method::*;

#[test]
fn gold_generic_method() {
    let c = Container::new(42);
    let result: SumResult = sum_all(&c);
    let _ = result.value();
}
