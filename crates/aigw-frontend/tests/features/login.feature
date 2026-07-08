Feature: Login Authentication

  Background:
    Given API endpoints are mocked

  Scenario: Successful login and redirect
    Given I am on the login page
    When I type "admin" into the username field
    And I type "sk-master-change-me" into the password field
    And I click the Sign In button
    Then I should see the dashboard home page
    And the sidebar should be visible

  Scenario: Login with empty fields shows validation
    Given I am on the login page
    When I click the Sign In button without entering credentials
    Then I should see an error message about invalid credentials

  Scenario: Login with wrong password shows error
    Given I am on the login page
    When I type "admin" into the username field
    And I type "wrong-password" into the password field
    Then I should not be redirected to the home page

  Scenario: Already authenticated user is redirected
    Given I am already authenticated via cookie
    When I visit "/dash/login"
    Then I should be redirected to "/dash/home"
