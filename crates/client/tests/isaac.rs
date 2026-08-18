#[test]
fn isaac_seed_1_2_3_4_first_eight() {
    let mut rng = client::io::Isaac::new(&[1, 2, 3, 4]);
    let got: Vec<i32> = (0..8).map(|_| rng.next_int()).collect();
    assert_eq!(
        got,
        [
            -621246914,
            1957022519,
            -1345000077,
            -2021884860,
            -1882702437,
            1616913581,
            -8779862,
            1337573575
        ]
    );
}
