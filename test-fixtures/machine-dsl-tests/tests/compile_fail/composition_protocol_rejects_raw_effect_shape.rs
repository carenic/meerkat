use meerkat_mob::generated::protocol_flow_loop_until_evaluation;

fn main() {
    let _ = protocol_flow_loop_until_evaluation::accept_evaluate_until_condition("not an effect");
}
