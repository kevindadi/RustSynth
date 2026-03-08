use bench_obl_trait_gate::*;

#[test]
fn gold_trait_gate() {
    let item = PrintableItem::new(42);
    let result: PrintResult = print_it(&item);
    let _ = result.value();
}
