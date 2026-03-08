use bench_obl_distractor_shape::*;

#[test]
fn gold_distractor_shape() {
    let valid = ValidInput::new(10);
    let output: ProcessedOutput = process(valid);
    let _ = output.get();
}
