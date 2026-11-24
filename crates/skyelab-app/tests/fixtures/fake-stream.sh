#!/bin/sh
# A long streamed answer, for fake-claude.sh's "stream" prompt (performance runs, docs/debugging.md): 1500 text
# deltas about 20 ms apart (~30 s at full speed) that build a markdown reply with headings, lists and code, then
# the complete message and the result. One awk process writes it all, so the pace is the sleeps, not forks.
awk 'BEGIN {
  n = 1500; text = ""
  for (i = 0; i < n; i++) {
    if (i % 100 == 0) w = sprintf("\\n\\n## Part %d\\n\\n", i / 100)
    else if (i % 50 == 0) w = "\\n\\n```rust\\nfn tail(c: &mut Cursor) -> Field {\\n    c.rest()\\n}\\n```\\n\\n"
    else if (i % 10 == 0) w = sprintf("\\n- step %d keeps the tail", i)
    else w = sprintf(" word%d", i)
    text = text w
    printf "{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"%s\"}},\"parent_tool_use_id\":null}\n", w
    fflush()
    system("sleep 0.02")
  }
  printf "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"%s\"}]},\"parent_tool_use_id\":null}\n", text
  printf "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"duration_ms\":30000,\"total_cost_usd\":0.2,\"result\":\"done\"}\n"
}'
