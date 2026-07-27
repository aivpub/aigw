@skip
Feature: Body Archive 读路径 + FileSystem 后端 — Stage 83

  # ━━━━ Stage 83: footer cache + FS backend + read-body error semantics ━━━━
  # 这些契约已由 `crates/aigw-core/tests/stage83_read_path.rs` 的 10 个
  # 红绿单元测试覆盖（footer cache / row group 定位 / NotFound vs Err /
  # S3 ${ENV_VAR} 占位符 / FileSystem 构造 / FS 归档写入分区路径 / FS 回源
  # 读回 body 一致）。此处保留为 BDD 文档场景，待后续 Stage 把读路径暴露
  # 为 admin/real-api 端点后取消 @skip 走 real BDD 三后端验收。
  # 对应审计 P1-1/P1-2/P1-3/P1-4/P2-10/P2-11/P2-12。

  Scenario: read_body NotFound 返回 Ok(None)
    Given 一个空的对象存储
    When BodyArchiver 从路径 "year=2026/month=07/day=25/hour=14/data.parquet" 读取 request_id "missing"
    Then 返回 Ok(None)

  Scenario: read_body 存储不可达返回 Err
    Given 一个总是失败的对象存储
    When BodyArchiver 从路径 "year=2026/month=07/day=25/hour=14/data.parquet" 读取 request_id "req-x"
    Then 返回 Err

  Scenario: S3 凭证 ${ENV_VAR} 占位符解析
    Given 环境变量 AIGW_TEST_AK=resolved-ak, AIGW_TEST_SK=resolved-sk, AIGW_TEST_BUCKET=env-bucket
    When 反序列化含占位符的 S3 配置
    Then bucket 解析为 "env-bucket"
    And access_key_id 解析为 "resolved-ak"
    And secret_access_key 解析为 "resolved-sk"

  Scenario: StorageBackend::FileSystem 构造 LocalFileSystem
    Given 配置 storage.backend=fs，path 为临时目录
    When 调用 build_object_store_for_backend
    Then 返回一个可用的 LocalFileSystem store
    And put+get 一段字节能 round-trip

  Scenario: 本地 FS 归档写入分区路径
    Given BodyArchiver 配置 storage.backend=fs，path 为临时目录
    When 归档 1 条 body 数据 hour=2026-07-25T14
    Then 返回路径为 "year=2026/month=07/day=25/hour=14/data.parquet"
    And 文件物理存在于该分区路径下

  Scenario: 本地 FS 归档回源读回 body 一致
    Given BodyArchiver 配置 storage.backend=fs，path 为临时目录
    And 已归档 1 条 body 数据 hour=2026-07-25T14 request_id="rt-001"
    When 从存储读取 request_id "rt-001"
    Then 返回 Some(body)，messages 内容为 "msg-rt-001"

  Scenario: footer cache 第二次查询命中跳过 footer 拉取
    Given 一个 InMemory 对象存储已写入含 2 条记录的 parquet
    When 第一次查询 request_id "req-001"
    Then 发生了至少一次 get_range 请求
    When 第二次查询 request_id "req-002"
    Then 第二次新增的 get_range 请求数少于第一次的总请求数
