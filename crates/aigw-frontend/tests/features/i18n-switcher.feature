Feature: i18n Language Switcher

  Scenario: First visit with Chinese browser shows Chinese sidebar menu
    Given I prepare Chinese browser locale
    And I clear the aigw-language storage
    When I load "/dash/usage"
    Then the sidebar should show "API 密钥" menu item

  Scenario: First visit with English browser shows English sidebar menu
    Given I prepare English browser locale
    And I clear the aigw-language storage
    When I load "/dash/usage"
    Then the sidebar should show "Virtual Keys" menu item

  Scenario: localStorage language preference overrides browser language
    Given I prepare Chinese browser locale
    And I pre-set localStorage "aigw-language" to "en"
    When I load "/dash/usage"
    Then the sidebar should show "Virtual Keys" menu item
