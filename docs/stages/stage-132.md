# Stage 132: Codex 多轮 tool 历史适配（Phase 52）

**所属**: Phase 52（Codex 客户端兼容）
**预估**: 6h（item 分派 + tool 配对归一 + UT/BDD + 文档）
**依赖**: Stage 131（工具归一化 + role 归一 + part 映射）
**状态**: ✅ 完成（代码 + UT + BDD + 门禁全绿 + 真实端到端；未提交/未部署）

---

## 1. 目标

Stage 131 修完单轮后，Codex 一旦调用工具就进入第二轮——`input[]` 里带上 `function_call`（assistant 的调用）与 `function_call_output`（执行结果）。Stage 131 的 `input_to_messages` **忽略 item 的 `type` 字段**，一律按 message 处理：调用变成了普通 user 消息、结果也没有 `tool_call_id`，模型看到的是残缺历史。

本次按 `type` 分派，并强制 Chat Completions 的 tool-call 不变量。

## 2. 现象与证据

用假 Responses-SSE 上游驱动 Codex 0.160.0 走完一轮真实工具往返（`exec_command` → `echo hello`），抓得 round-2 body：

```json
{"input": [
  {"type":"message","role":"developer","content":[...]},
  {"type":"message","role":"user","content":[...]},
  {"type":"function_call","id":"fc_1","call_id":"call_abc123",
   "name":"exec_command","arguments":"{\"cmd\":\"echo hello\"}"},
  {"type":"function_call_output","id":"fco_...","call_id":"call_abc123",
   "output":"Chunk ID: 9a26e1\nProcess exited with code 0\nOutput:\nhello\n"}
]}
```

即 `input[]` 是**带标签的联合体**，不是纯消息列表。

## 3. 方案

### 3.1 `items_to_messages` — 按 `type` 分派

| item type | 转换为 |
|-----------|--------|
| `message` / 无 type | 普通消息（content parts 归一，`input_text`→`text`） |
| `reasoning` | 暂存（`summary[].text` 拼接），附到下一条 assistant |
| `function_call` / `custom_tool_call` / `tool_search_call` | assistant 的 `tool_calls`（并行调用合并进**同一条** assistant） |
| `function_call_output` / `custom_tool_call_output` / `tool_search_output` | `role="tool"` + `tool_call_id`（对象/数组输出字符串化） |
| 裸 `input_text` / `text` | user 消息 |
| 未知（`web_search_call` / `local_shell_call` / `file_search_call` …） | **跳过**（无 Chat 对应；插入会在 assistant 的 tool_calls 与其回复之间夹一条消息，strict 上游拒收） |

细节：
- **命名空间调用**：`function_call` 带 `namespace` 字段时，用与请求侧**同一规则**拍平为 `{ns}__{child}`——否则模型看到的是它从未被声明过的工具名。
- **`custom` 调用**：free-form `input` 包成 `{"input": ...}`，与请求侧 `custom` 工具降级后的 schema 对齐。
- **`tool_search` 调用**：名称固定为 `tool_search` 代理名（不可改名，客户端按此名调用）。

### 3.2 `normalize_tool_pairing` — 强制 tool-call 不变量

Chat Completions schema 要求：带 `tool_calls` 的 assistant 消息**必须**紧接每个 `tool_call_id` 各一条 `tool` 消息。Codex 历史会违反（中途重连留下悬空调用、回复的宣布者被裁掉）。

处理：未应答的 `tool_call` 剪除；只剩空壳（无 content 无 call）的 assistant 丢弃；孤立 `tool` 回复丢弃。

### 3.3 设计参考

`~/works/play/sub2api` 的 `buildChatMessagesFromItems`（`chatcompletions_responses_bridge.go:187-215`）+ `normalizeChatMessages`（`:576-640`）——同为 Codex 场景打磨过，语义一致。

## 4. TDD

10 个新 UT（`crates/aigw-core/src/adapter.rs`）：

- `..._item_function_call_becomes_assistant_tool_calls`（基本往返）
- `..._item_parallel_calls_merge_into_one_assistant`
- `..._item_unanswered_call_dropped`
- `..._item_orphan_tool_output_dropped`
- `..._item_reasoning_attaches_to_assistant`
- `..._item_unknown_type_skipped`
- `..._item_namespace_qualified_call_flattened`
- `..._item_custom_tool_call_wraps_input`
- `..._item_object_output_stringified`
- `..._multiturn_codex_fixture_end_to_end`（真实抓包固化）

BDD：`responses.feature` 新增「multi-turn tool history」场景（断言上游收到 `role="tool"` 与 `role="assistant"`）。

## 5. 回归验证

1. `task test` — aigw-core **527** UT 全绿
2. `task bdd` — **284 场景（271 pass / 13 skip）/ 1443 steps**
3. `task fmt` / `task lint` green
4. 真实上游：round-2 变换 payload → **200**

## 6. 门禁

- [x] 10 个新 UT 通过
- [x] 多轮 fixture 回归 UT 通过
- [x] `task test` / `task bdd` / `task fmt` / `task lint` 全绿
- [x] 真实端到端：Codex 0.160.0 完成一轮 tool 往返；变换后 payload 打真实上游 200
- [ ] git commit（精确 add；`--signoff`）

## 7. 不做（边界）

- **`tool_choice` 的 `{type:"namespace"}` 形态**（TD-017b）
- **内建搜索执行**（TD-017c）
- **streaming SSE 事件映射 UT**（TD-017d 剩余部分）
