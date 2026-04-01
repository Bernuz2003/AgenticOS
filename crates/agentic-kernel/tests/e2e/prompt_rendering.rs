use agentic_kernel::test_support::prompt_rendering::render_qwen_initial_prompt_with_template;

#[test]
fn qwen_jinja_template_disables_thinking_in_generation_prompt() {
    let rendered = render_qwen_initial_prompt_with_template(
        "{% for message in messages %}<|im_start|>{{ message.role }}\n{{ message.content }}<|im_end|>\n{% endfor %}{% if add_generation_prompt %}<|im_start|>assistant\n{% if enable_thinking is defined and enable_thinking is false %}<think>\n\n</think>\n\n{% endif %}{% endif %}",
        "policy",
        "question",
    );

    assert!(rendered.contains("<|im_start|>assistant\n<think>\n\n</think>\n\n"));
}
