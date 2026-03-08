use bench_obl_assoc_decides::*;

#[test]
fn gold_assoc_decides() {
    let d = Doubler::new(21);
    let result: TransformResult = apply_transform(&d);
    let _ = result.value();
}
