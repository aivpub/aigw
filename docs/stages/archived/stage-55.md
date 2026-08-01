# Stage 55: Models 管理页面完整 CRUD 前端

**Phase**: 19 — UI Enhancement（Models CRUD + Spend Logs 可视化）
**状态**: ⏳ 待开始
**预估**: 7-8h（上调，因 credential 联动 + 定价转换逻辑）
**依赖**: 无（后端 CRUD 接口 + credential/list 接口已就绪）

---

## 目标

1. **新增模型** — 结构化表单创建新的 proxy_models 记录
2. **编辑模型** — 对话框预填现有数据，修改后更新
3. **删除模型** — 确认对话框 + 国际化提示
4. **错误处理** — 网络错误 toast + 后端校验错误展示

## 核心设计原则（按反馈确认）

### Model Name 即 Model Group

- `model_name` 字段即为 litellm 中的 model group 名称（代理对外暴露的模型名）
- **上游 model** 字段默认自动填写为 `model_name` 相同的值（跟随 model_name 变化），允许用户手动编辑
- 用户可将其改为与 model_name 不同的值（如 model_name=`my-gpt4`，上游 model=`openai/gpt-4`）

### API Key 与 Credential 二选一

- `api_base` + `api_key` 为直接配置方式
- `litellm_credential_name` 为引用 credential 方式
- 两种方式互斥，通过 Radio/Tab 切换
- credential 下拉数据来自 `GET /credential/list`
- credential 下拉框旁有 "+" 按钮，新开 tab 跳转到 `/#/credentials` 页面（后续可加）

### 定价输入：每百万 Token 美元价格

- 用户输入的是 **每百万 token 的美元单价**（如 GPT-4 输入 $30，输出 $60）
- 前端在提交时转换为 `input_cost_per_token`（除以 1,000,000）
- 编辑预填时反向转换（乘以 1,000,000）
- 列表中的 Cost 列已经是转换后的 per-token 价格再乘以 1M 显示，逻辑不变

## 验收标准

- [ ] "Add Model" 按钮打开结构化表单对话框
- [ ] model_name（必填）作为 model_group 名称
- [ ] 上游 model 字段默认跟随 model_name（实时联动），允许手动编辑
- [ ] custom_llm_provider 下拉选择（openai/anthropic/deepseek/…）
- [ ] **API Key 模式**：填写 api_base + api_key
- [ ] **Credential 模式**：从 credential 下拉选择 + 旁边 "+" 新建快捷入口
- [ ] 两种模式互斥切换，切换时清空另一模式的字段
- [ ] 价格输入：**每百万 token 美元单价**，两个字段：Input Price ($/1M tokens) / Output Price ($/1M tokens)
- [ ] rpm / tpm 速率限制输入
- [ ] 提交时自动转换：`input_cost_per_token = InputPrice / 1_000_000`
- [ ] 编辑时自动预填反向转换（token单价 → 百万token单价）
- [ ] 删除时弹出确认→ `DELETE /model/delete?model_id=...` → 刷新列表
- [ ] 错误消息 toast 展示（网络错误 + 后端校验错误）
- [ ] 移动端 card 操作按钮
- [ ] **门禁**: 全量 UT + BDD + 前端 Playwright

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-frontend/src/pages/models/index.tsx` | **修改** — Add/Edit/Delete 按钮 + 对话框集成 |
| `crates/aigw-frontend/src/pages/models/ModelDialog.tsx` | **新建** — 新增/编辑模型表单对话框 |
| `crates/aigw-frontend/src/pages/models/DeleteConfirm.tsx` | **新建** — 删除确认对话框 |
| `crates/aigw-frontend/src/lib/api.ts` | **修改** — 确保 apiGet 被 ModelDialog 正确调用 |

## 技术方案

### 1. ModelDialog 表单（含反馈后的设计）

```tsx
// 上游 provider 与认证模式
type AuthMode = "api_key" | "credential";

interface ModelFormData {
  model_name: string;              // 必填，即 model group 名称
  upstream_model: string;          // 上游模型标识，默认 = model_name，可编辑
  custom_llm_provider: string;     // 下拉: openai/anthropic/deepseek/…
  auth_mode: AuthMode;             // api_key 或 credential
  api_base: string;                // 仅 auth_mode=api_key 时
  api_key: string;                 // 仅 auth_mode=api_key 时
  credential_name: string;         // 仅 auth_mode=credential 时
  rpm: number | null;
  tpm: number | null;
  input_price_per_million: number | null;   // 用户输入：$/1M tokens
  output_price_per_million: number | null;  // 用户输入：$/1M tokens
}
```

### 2. 上游 Model 名称联动

```tsx
function ModelDialog() {
  const [modelName, setModelName] = useState("");
  const [upstreamModel, setUpstreamModel] = useState("");
  const [upstreamManuallyEdited, setUpstreamManuallyEdited] = useState(false);

  // 当 model_name 变化且用户未手动编辑过 upstream_model 时，自动跟随
  function handleModelNameChange(value: string) {
    setModelName(value);
    if (!upstreamManuallyEdited) {
      setUpstreamModel(value);  // 自动填充
    }
  }

  // 用户手动编辑 upstream_model 后标记
  function handleUpstreamModelChange(value: string) {
    setUpstreamModel(value);
    setUpstreamManuallyEdited(true);
  }

  // JSX
  return (
    <>
      <FormField label="Model Name" required>
        <Input value={modelName} onChange={e => handleModelNameChange(e.target.value)}
               placeholder="my-gpt-4" />
        <FormDescription>
          Proxy model group name. Upstream model auto-fills with this value.
        </FormDescription>
      </FormField>
      <FormField label="Upstream Model" required>
        <Input value={upstreamModel} onChange={e => handleUpstreamModelChange(e.target.value)} />
        <FormDescription>
          Auto-filled from Model Name. Edit to override (e.g. openai/gpt-4).
        </FormDescription>
      </FormField>
      {/* ... */}
    </>
  );
}
```

### 3. API Key / Credential 二选一

```tsx
function AuthModeSwitch({ mode, onChange }: {
  mode: AuthMode;
  onChange: (m: AuthMode) => void;
}) {
  return (
    <div className="flex items-center gap-2 mb-3">
      <Label className="text-xs font-medium">Authentication</Label>
      <Tabs value={mode} onValueChange={(v) => onChange(v as AuthMode)}>
        <TabsList className="h-7">
          <TabsTrigger value="api_key" className="text-xs h-6">API Key</TabsTrigger>
          <TabsTrigger value="credential" className="text-xs h-6">Credential</TabsTrigger>
        </TabsList>
      </Tabs>
    </div>
  );
}

function ApiKeyFields({ api_base, api_key, onChange }: {...}) {
  return (
    <div className="space-y-3">
      <FormField label="API Base">
        <Input value={api_base} placeholder="https://api.openai.com/v1" />
      </FormField>
      <FormField label="API Key">
        <Input type="password" value={api_key} placeholder="sk-..." />
      </FormField>
    </div>
  );
}

function CredentialFields({ credential_name, onChange }: {...}) {
  const { data } = useQuery({
    queryKey: ["credentials-list"],
    queryFn: () => apiGet("/credential/list"),
  });
  const credentials = data?.data ?? [];

  return (
    <div className="flex items-end gap-2">
      <FormField label="Credential" className="flex-1">
        <Select value={credential_name} onValueChange={onChange}>
          <SelectTrigger><SelectValue placeholder="Select credential…" /></SelectTrigger>
          <SelectContent>
            {credentials.map((c: any) => (
              <SelectItem key={c.credential_name} value={c.credential_name}>
                {c.credential_name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </FormField>
      <Button variant="outline" size="icon" className="h-9 w-9" title="New credential"
              onClick={() => window.open("/#/credentials", "_blank")}>
        <Plus className="h-4 w-4" />
      </Button>
    </div>
  );
}
```

切换时清空逻辑：
```tsx
function handleAuthModeChange(mode: AuthMode) {
  setAuthMode(mode);
  if (mode === "credential") {
    // 清空 api_base + api_key，提交时通过 credential 解析
    setApiBase("");
    setApiKey("");
  } else {
    setCredentialName("");
  }
}
```

### 4. 定价输入与转换

```tsx
// 用户输入层：每百万 token 美元单价
<FormField label="Input Price ($/1M tokens)">
  <Input type="number" step="0.0001" min="0"
         value={inputPricePerMillion ?? ""}
         placeholder="30.00" />
  <FormDescription>GPT-4: $30/1M input tokens</FormDescription>
</FormField>

<FormField label="Output Price ($/1M tokens)">
  <Input type="number" step="0.0001" min="0"
         value={outputPricePerMillion ?? ""}
         placeholder="60.00" />
  <FormDescription>GPT-4: $60/1M output tokens</FormDescription>
</FormField>
```

提交时转换：
```ts
function submitBody(form: ModelFormData): object {
  const inputCostPerToken = form.input_price_per_million != null
    ? form.input_price_per_million / 1_000_000
    : undefined;
  const outputCostPerToken = form.output_price_per_million != null
    ? form.output_price_per_million / 1_000_000
    : undefined;

  const litellm_params: Record<string, unknown> = {
    model: form.upstream_model,
    custom_llm_provider: form.custom_llm_provider || undefined,
    rpm: form.rpm ?? undefined,
    tpm: form.tpm ?? undefined,
    input_cost_per_token: inputCostPerToken,
    output_cost_per_token: outputCostPerToken,
  };

  if (form.auth_mode === "api_key") {
    litellm_params.api_base = form.api_base || undefined;
    litellm_params.api_key = form.api_key || undefined;
  } else {
    litellm_params.litellm_credential_name = form.credential_name || undefined;
  }

  return {
    model_name: form.model_name,
    litellm_params,
    model_info: {
      input_cost_per_token: inputCostPerToken,
      output_cost_per_token: outputCostPerToken,
    },
  };
}
```

编辑预填（反向转换）：
```ts
function populateFormFromModel(m: ModelItem): ModelFormData {
  const p = m.litellm_params as Record<string, unknown>;
  const info = m.model_info as Record<string, unknown>;

  // token 单价 → 百万 token 单价
  const rawInput = (info.input_cost_per_token as number) ?? (p.input_cost_per_token as number);
  const rawOutput = (info.output_cost_per_token as number) ?? (p.output_cost_per_token as number);
  const inputPrice = rawInput != null ? rawInput * 1_000_000 : null;
  const outputPrice = rawOutput != null ? rawOutput * 1_000_000 : null;

  // 在直接配置和 credential 间判定
  const hasCredential = !!(p.litellm_credential_name as string);

  return {
    model_name: m.model_name,
    upstream_model: (p.model as string) || m.model_name,
    custom_llm_provider: (p.custom_llm_provider as string) || "",
    auth_mode: hasCredential ? "credential" : "api_key",
    api_base: (p.api_base as string) || "",
    api_key: (p.api_key as string) || "",
    credential_name: (p.litellm_credential_name as string) || "",
    rpm: (p.rpm as number) || null,
    tpm: (p.tpm as number) || null,
    input_price_per_million: inputPrice,
    output_price_per_million: outputPrice,
  };
}
```

### 5. 删除确认

```tsx
<AlertDialog>
  <AlertDialogTrigger asChild>
    <Button variant="ghost" size="icon"><Trash2 /></Button>
  </AlertDialogTrigger>
  <AlertDialogContent>
    <AlertDialogTitle>Delete Model</AlertDialogTitle>
    <AlertDialogDescription>
      Delete proxy model "{model.model_name}"? This action cannot be undone.
      Existing API keys that reference this model will no longer have access.
    </AlertDialogDescription>
    <AlertDialogFooter>
      <AlertDialogCancel>Cancel</AlertDialogCancel>
      <AlertDialogAction onClick={handleDelete}>Delete</AlertDialogAction>
    </AlertDialogFooter>
  </AlertDialogContent>
</AlertDialog>
```

## TDD 测试用例

### BDD (Gherkin)

```gherkin
Scenario: Create model — upstream model auto-fills from model name
  Given 在 models 页面点击 "Add Model"
  When 在 model_name 输入 "my-gpt-4"
  Then 上游 model 字段自动填充为 "my-gpt-4"

Scenario: Upstream model can differ from model name
  Given model_name = "my-gpt-4", 上游 model 已自动填充为 "my-gpt-4"
  When 用户将上游 model 修改为 "openai/gpt-4"
  Then 上游 model 保持为 "openai/gpt-4"

Scenario: Auth mode switch clears unrelated fields
  Given auth_mode = "api_key", 已填写 api_base 和 api_key
  When 切换到 "credential" 模式
  Then api_base 和 api_key 被清空
  And credential 下拉可用

Scenario: Credential dropdown shows available credentials
  Given 有 credential 列表数据
  When 打开 credential 下拉
  Then 显示所有可用 credential 名称

Scenario: Pricing — per-million token input converts to per-token
  Given 填写 Input Price = "30" ($/1M tokens)
  When 提交表单
  Then litellm_params.input_cost_per_token = 0.00003

Scenario: Edit model — pricing reversed from per-token to per-million
  Given 现有模型的 input_cost_per_token = 0.00003
  When 点击 Edit 按钮
  Then 表单 Input Price 预填 "30"

Scenario: Delete model with confirmation
  Given 列表中有一个模型
  When 点击 Delete → 确认
  Then 模型从列表消失
```

## 风险与回滚

| 风险 | 应对 |
|------|------|
| credential/list 接口返回空 | 下拉显示 "No credentials configured"，旁边 "+" 按钮引导用户新建 |
| 上游 model 联动逻辑与用户预期不一致 | 首次手动编辑时标记 `upstreamManuallyEdited`，之后不再自动覆盖 |
| 价格浮点精度 | 提交时 `parseFloat((price / 1_000_000).toFixed(10))`，保证 10 位小数精度 |
| 编辑时原 api_key 已加密不可读 | 预填 `"***"` 占位，提交时如果用户未修改则不上传（保留原值） |

回滚方式：`git revert` 该 commit。
