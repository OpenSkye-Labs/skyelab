#!/bin/sh
# One turn with every rich timeline row, for fake-claude.sh's "timeline" prompt (frame shapes from recorded Claude
# Code 2.1.x transcripts): streamed thinking and text, todos updated in place, an edit, bash output, usage; then a
# usage limit, a proposed plan and a question, left waiting.
d() { printf '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"%s","%s":"%s"}},"parent_tool_use_id":null}\n' "$1" "$2" "$3"; sleep 0.15; }
a() { printf '{"type":"assistant","message":{"role":"assistant","content":[%s]},"parent_tool_use_id":null}\n' "$1"; }
u() { printf '{"type":"user","message":{"role":"user","content":[%s]},"parent_tool_use_id":null}\n' "$1"; }
for w in "The user wants" " a parser for the" " new stream." " Start with the" " event types."; do d thinking_delta thinking "$w"; done
a '{"type":"thinking","thinking":"The user wants a parser for the new stream. Start with the event types.","signature":"s"}'
for w in "I'll add" " the parser," " then test" " it."; do d text_delta text "$w"; done
# Hold mid-stream long enough for a loaded test machine to see the partial text.
sleep 2
a '{"type":"text","text":"I'"'"'ll add the parser, then test it."}'
a '{"type":"tool_use","id":"toolu_t1","name":"TodoWrite","input":{"todos":[{"content":"Add the event types","status":"in_progress","activeForm":"Adding"},{"content":"Parse the stream","status":"pending","activeForm":"Parsing"},{"content":"Run the tests","status":"pending","activeForm":"Testing"}]}}'
u '{"type":"tool_result","tool_use_id":"toolu_t1","content":"Todos updated"}'
a '{"type":"tool_use","id":"toolu_e1","name":"Edit","input":{"file_path":"'"$(pwd -P)"'/src/event.rs","old_string":"pub enum Event {\n    Init,\n    Result,\n}","new_string":"pub enum Event {\n    Init,\n    Delta { text: String },\n    Result,\n}","replace_all":false}}'
u '{"type":"tool_result","tool_use_id":"toolu_e1","content":"The file has been updated."}'
a '{"type":"tool_use","id":"toolu_b1","name":"Bash","input":{"command":"cargo test -p parser","description":"Run the tests"}}'
u '{"type":"tool_result","tool_use_id":"toolu_b1","content":"\u001b[32mrunning 3 tests\u001b[0m\ntest parses_deltas ... ok\ntest parses_usage ... ok\ntest parses_limits ... ok\n\ntest result: ok. 3 passed","is_error":false}'
a '{"type":"tool_use","id":"toolu_t2","name":"TodoWrite","input":{"todos":[{"content":"Add the event types","status":"completed","activeForm":"Adding"},{"content":"Parse the stream","status":"completed","activeForm":"Parsing"},{"content":"Run the tests","status":"in_progress","activeForm":"Testing"}]}}'
u '{"type":"tool_result","tool_use_id":"toolu_t2","content":"Todos updated"}'
a '{"type":"text","text":"Done. The new variant:\n\n```rust\nDelta { text: String },\n```\n\nSee [event.rs](src/event.rs)."}'
printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":4210,"total_cost_usd":0.1667,"result":"Done.","usage":{"input_tokens":4,"cache_creation_input_tokens":13823,"cache_read_input_tokens":256120,"output_tokens":175}}\n'
printf '{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour","resetsAt":%s}}\n' "$(( $(date +%s) + 8040 ))"
printf '{"type":"control_request","request_id":"p-2","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"## Plan\\n\\n1. Add `Event::Delta`\\n2. Parse `stream_event` frames\\n3. Show them as they stream"},"tool_use_id":"toolu_x2"}}\n'
printf '{"type":"control_request","request_id":"q-2","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"questions":[{"question":"Where should streamed text go?","header":"Storage","options":[{"label":"Show only","description":"The full block is stored"},{"label":"Store every delta","description":"Bigger database"}],"multiSelect":false},{"question":"Which cards next?","header":"Cards","options":[{"label":"Subagents","description":"Task rows"},{"label":"Images","description":"Inline viewer"}],"multiSelect":true}]},"tool_use_id":"toolu_q2"}}\n'
