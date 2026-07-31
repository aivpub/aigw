Feature: Login Authentication

  Scenario: Successful login and redirect
    Given I am on the login page
    When I type "admin" into the username field
    And I type "sk-master-change-me" into the password field
    And I click the Sign In button
    Then I should see the usage page
    And the sidebar should be visible

  Scenario: Login with empty fields shows validation
    Given I am on the login page
    When I click the Sign In button without entering credentials
    Then I should see an error message about invalid credentials

  Scenario: Login with wrong password shows error
    Given I am on the login page
    When I type "admin" into the username field
    And I type "wrong-password" into the password field
    And I click the Sign In button
    Then I should not be redirected to the usage page

  Scenario: Already authenticated user is redirected
    Given I am already authenticated via cookie
    When I visit "/dash/login"
    Then I should be redirected to "/dash/usage"

  Scenario: API 401 triggers redirect to login
    Given I am authenticated and on the usage page
    When the API returns 401 for spend/logs request
    Then I should be redirected to "/dash/login"
    And the sidebar should not be visible

  @skip
  Scenario: 401 redirect preserves current page path
    Given I am authenticated and on "/dash/keys"
    When the API returns 401 for key/list request
    Then I should be redirected to "/dash/login"
    And the URL should contain "redirect=%2Fdash%2Fkeys"

  Scenario: Login after 401 redirect returns to original page
    Given I was redirected to "/dash/login?redirect=%2Fdash%2Fmodels"
    When I type "admin" into the username field
    And I type "sk-master-change-me" into the password field
    And I click the Sign In button
    Then I should be redirected to "/dash/models"
