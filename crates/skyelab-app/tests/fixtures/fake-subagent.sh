#!/bin/sh
# One turn with two subagents, for fake-claude.sh's "subagent" prompt. Frame shapes are T3's recorded Claude
# transcripts (testkit fixtures `subagent` and `claude_nested_subagent_model`): an Agent tool call, its task_started,
# the subagent's own messages under parent_tool_use_id, task_progress, task_notification, then the call's result.
# The second subagent holds 4 s mid-run so a live check can see it running (not with a .fake-subagent-quick file).
a() { printf '{"type":"assistant","message":{"role":"assistant","content":[%s]},"parent_tool_use_id":%s}\n' "$1" "${2:-null}"; }
u() { printf '{"type":"user","message":{"role":"user","content":%s},"parent_tool_use_id":%s}\n' "$1" "${2:-null}"; }
s() { printf '{"type":"system","subtype":"%s","task_id":"%s","tool_use_id":"%s",%s,"uuid":"u","session_id":"fake-sess-1"}\n' "$1" "$2" "$3" "$4"; }
a '{"type":"text","text":"Spawning one subagent per file."}'
a '{"type":"tool_use","id":"toolu_sa","name":"Agent","input":{"description":"Read package.json","prompt":"Read package.json and report its scripts.","subagent_type":"general-purpose"}}'
s task_started a35c toolu_sa '"description":"Read package.json","subagent_type":"general-purpose","task_type":"local_agent"'
u '"Read package.json and report its scripts."' '"toolu_sa"'
a '{"type":"tool_use","id":"toolu_sb","name":"Agent","input":{"description":"Audit src/math.ts","prompt":"Run ls and wc -l on src/math.ts.","subagent_type":"Explore"}}'
s task_started a3d3 toolu_sb '"description":"Audit src/math.ts","subagent_type":"Explore","task_type":"local_agent"'
u '"Run ls and wc -l on src/math.ts."' '"toolu_sb"'
s task_progress a35c toolu_sa '"description":"Reading package.json","usage":{"total_tokens":8707,"tool_uses":1,"duration_ms":2053},"last_tool_name":"Read"'
a '{"type":"tool_use","id":"toolu_r1","name":"Read","input":{"file_path":"package.json"}}' '"toolu_sa"'
u '[{"type":"tool_result","tool_use_id":"toolu_r1","content":"1\t{\"name\":\"demo\",\"scripts\":{\"test\":\"tsc\"}}"}]' '"toolu_sa"'
s task_notification a35c toolu_sa '"status":"completed","summary":"package.json has one script: test runs tsc.","usage":{"total_tokens":10755,"tool_uses":1,"duration_ms":6211}'
u '[{"type":"tool_result","tool_use_id":"toolu_sa","content":[{"type":"text","text":"package.json has one script: test runs tsc."}]}]'
a '{"type":"tool_use","id":"toolu_b1","name":"Bash","input":{"command":"ls -la src && wc -l src/math.ts","description":"Inspect math.ts"}}' '"toolu_sb"'
s task_progress a3d3 toolu_sb '"description":"Running Inspect math.ts","usage":{"total_tokens":54592,"tool_uses":1,"duration_ms":4146},"last_tool_name":"Bash"'
[ -f .fake-subagent-quick ] || sleep 4
u '[{"type":"tool_result","tool_use_id":"toolu_b1","content":"math.ts\n1 src/math.ts","is_error":false}]' '"toolu_sb"'
s task_notification a3d3 toolu_sb '"status":"completed","summary":"math.ts is one line: export const add.","usage":{"total_tokens":58554,"tool_uses":1,"duration_ms":9822}'
u '[{"type":"tool_result","tool_use_id":"toolu_sb","content":[{"type":"text","text":"math.ts is one line: export const add."}]}]'
a '{"type":"text","text":"Both subagents finished: package.json runs tsc, and math.ts exports add."}'
printf '{"type":"result","subtype":"success","is_error":false,"duration_ms":10400,"total_cost_usd":0.0312,"result":"done","usage":{"input_tokens":120,"output_tokens":80}}\n'
