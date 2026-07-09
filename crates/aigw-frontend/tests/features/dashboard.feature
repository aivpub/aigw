Feature: Usage Overview

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Usage page

  Scenario: View spend overview cards
    Then I should see total spend information
    And I should see spend by model chart or data

  Scenario: Loading state shows skeleton
    Given API endpoints are slow to respond
    When I visit the Usage page
    Then I should see loading indicators before data appears
