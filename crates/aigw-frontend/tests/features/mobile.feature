Feature: Mobile Responsive Layout

  Background:
    Given API endpoints are mocked

  Scenario: Mobile sidebar toggle
    Given I am logged in as admin
    And the viewport is mobile size 375x667
    When I visit "/dash/usage"
    Then the sidebar should be hidden
    When I click the hamburger menu button
    Then the sidebar should be visible
    When I click the overlay backdrop
    Then the sidebar should be hidden again

  Scenario: Mobile key list uses card layout
    Given I am logged in as admin
    And the viewport is mobile size 375x667
    When I visit "/dash/keys"
    Then the key data should be displayed in a mobile-friendly format

  Scenario: Mobile usage stacks charts vertically
    Given I am logged in as admin
    And the viewport is mobile size 375x667
    When I visit "/dash/usage"
    Then the charts should fit within the mobile screen width
