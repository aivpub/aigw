Feature: Claude OAuth Credentials — CredentialsTab 入口（Stage 129）

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Models page
    And I click the "Credentials" tab on the Models page

  Scenario: OAuth 凭证在列表中以独立入口展示（active 状态 + 到期时间 + 绑定代理）
    Then I should see the OAuth credential "oauth-personal" with active status
    And I should see the OAuth expiry and bound proxy for "oauth-personal"

  Scenario: 打开 OAuth 对话框提交 cookie → 凭证出现
    When I click the New OAuth Credential button
    And I fill the OAuth dialog with name "oauth-new" cookie "sk-ant-sid-test-abc" proxy "hk-residential"
    And I submit the OAuth dialog
    Then the OAuth dialog closes
    And I should see the OAuth credential "oauth-new" with active status

  Scenario: needs_reauth 凭证展示红色徽章 + 上次错误 + Re-auth 按钮
    Given the OAuth credential "oauth-personal" requires re-auth
    Then I should see the OAuth credential "oauth-personal" with needs-reauth badge
    And I should see the OAuth last error for "oauth-personal"
    And I should see a "Re-auth" button for "oauth-personal"

  Scenario: 点击 Refresh 按钮 → 状态更新
    When I click the Refresh button on the OAuth credential "oauth-personal"
    Then the OAuth refresh request is sent for "oauth-personal"

  Scenario: Re-auth 重新粘贴 cookie → 恢复 active
    Given the OAuth credential "oauth-personal" requires re-auth
    When I click the "Re-auth" button on the OAuth credential "oauth-personal"
    And I paste a new cookie into the re-auth dialog
    And I submit the re-auth dialog
    Then the re-auth dialog closes

  Scenario: 敏感字段（token/session）不展示
    Then the OAuth credential "oauth-personal" shows no raw token values
