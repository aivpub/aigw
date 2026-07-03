//! OpenAPI 3.1 specification generator for aigw
//!
//! Generates an OpenAPI 3.1.0 specification as a `serde_json::Value`
//! using the `serde_json::json!()` macro. The spec describes all
//! existing endpoints, their request/response schemas, query parameters,
//! authentication requirements, and error responses.

use serde_json::{json, Value};

/// Generate the complete OpenAPI 3.1.0 specification as a `serde_json::Value`.
///
/// The spec covers all 18 endpoints across four tag groups:
/// - "Key Management" (6 endpoints)
/// - "Spend & Usage" (7 endpoints)
/// - "Chat & Models" (2 endpoints)
/// - "Health" (3 endpoints)
pub fn generate_openapi_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "aigw AI Gateway API",
            "description": "Rust-based litellm-compatible AI Gateway — OpenAI-compatible chat completions, virtual key management, and usage tracking.",
            "version": "0.1.0",
            "contact": {
                "name": "aigw project",
                "url": "https://github.com/aivpub/aigw"
            },
            "license": {
                "name": "MIT"
            }
        },
        "servers": [
            {
                "url": "http://localhost:4000",
                "description": "Local development server"
            }
        ],
        "tags": [
            {
                "name": "Key Management",
                "description": "Virtual API key generation, lookup, listing, update, deletion, and regeneration"
            },
            {
                "name": "Spend & Usage",
                "description": "Usage tracking — spend logs, per-key spend, per-user spend, per-tag spend, and global admin endpoints"
            },
            {
                "name": "Chat & Models",
                "description": "OpenAI-compatible chat completions and model listing"
            },
            {
                "name": "Health",
                "description": "Health check endpoints"
            }
        ],
        "paths": {
            "/key/generate": {
                "post": key_generate_spec()
            },
            "/key/info": {
                "get": key_info_spec()
            },
            "/key/list": {
                "get": key_list_spec()
            },
            "/key/update": {
                "put": key_update_spec()
            },
            "/key/delete": {
                "delete": key_delete_spec()
            },
            "/key/regenerate": {
                "post": key_regenerate_spec()
            },
            "/spend/logs": {
                "get": spend_logs_spec()
            },
            "/spend/keys": {
                "get": spend_keys_spec()
            },
            "/spend/users": {
                "get": spend_users_spec()
            },
            "/spend/tags": {
                "get": spend_tags_spec()
            },
            "/global/spend": {
                "get": global_spend_spec()
            },
            "/global/spend/logs": {
                "get": global_spend_logs_spec()
            },
            "/global/spend/keys": {
                "get": global_spend_keys_spec()
            },
            "/v1/chat/completions": {
                "post": chat_completions_spec()
            },
            "/v1/models": {
                "get": models_list_spec()
            },
            "/health": {
                "get": health_spec()
            },
            "/health/readiness": {
                "get": health_readiness_spec()
            },
            "/health/liveliness": {
                "get": health_liveliness_spec()
            }
        },
        "components": {
            "securitySchemes": {
                "BearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "API key or master key",
                    "description": "Authenticate using an API key generated via /key/generate, or the master key configured at startup."
                }
            },
            "schemas": {
                "ErrorResponse": {
                    "type": "object",
                    "required": ["error"],
                    "properties": {
                        "error": {
                            "type": "object",
                            "required": ["message", "type"],
                            "properties": {
                                "message": {
                                    "type": "string",
                                    "description": "Human-readable error description"
                                },
                                "type": {
                                    "type": "string",
                                    "description": "Machine-readable error type identifier"
                                }
                            }
                        }
                    }
                },
                "GenerateKeyRequest": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "Custom key value (sk- prefix). If omitted, a random key is generated." },
                        "key_alias": { "type": "string", "description": "Human-readable alias for the key" },
                        "user_id": { "type": "string" },
                        "team_id": { "type": "string" },
                        "organization_id": { "type": "string" },
                        "project_id": { "type": "string" },
                        "models": { "type": "array", "items": { "type": "string" }, "description": "Allowed model IDs" },
                        "max_budget": { "type": "number", "format": "float", "description": "Maximum spend budget" },
                        "budget_duration": { "type": "string", "description": "Budget reset interval (e.g. 1d, 1mo)" },
                        "tpm_limit": { "type": "integer", "format": "int64", "description": "Tokens per minute limit" },
                        "rpm_limit": { "type": "integer", "format": "int64", "description": "Requests per minute limit" },
                        "max_parallel_requests": { "type": "integer", "description": "Maximum concurrent requests" },
                        "expires": { "type": "string", "format": "date-time", "description": "Key expiration timestamp (RFC3339)" },
                        "metadata": { "type": "object", "description": "Arbitrary metadata" },
                        "permissions": { "type": "object", "description": "Permission configuration" },
                        "auto_rotate": { "type": "boolean", "description": "Enable automatic key rotation" },
                        "rotation_interval": { "type": "string", "description": "Rotation interval (e.g. 30d)" }
                    }
                },
                "KeyResponse": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "Raw API key value (sk- prefix)" },
                        "key_name": { "type": "string", "nullable": true },
                        "key_alias": { "type": "string", "nullable": true },
                        "token": { "type": "string", "nullable": true, "description": "SHA256 hash of the key" },
                        "user_id": { "type": "string", "nullable": true },
                        "team_id": { "type": "string", "nullable": true },
                        "organization_id": { "type": "string", "nullable": true },
                        "project_id": { "type": "string", "nullable": true },
                        "models": { "type": "array", "items": { "type": "string" } },
                        "max_budget": { "type": "number", "format": "float", "nullable": true },
                        "budget_duration": { "type": "string", "nullable": true },
                        "budget_reset_at": { "type": "string", "format": "date-time", "nullable": true },
                        "tpm_limit": { "type": "integer", "format": "int64", "nullable": true },
                        "rpm_limit": { "type": "integer", "format": "int64", "nullable": true },
                        "max_parallel_requests": { "type": "integer", "nullable": true },
                        "spend": { "type": "number", "format": "float" },
                        "expires": { "type": "string", "format": "date-time", "nullable": true },
                        "blocked": { "type": "boolean", "nullable": true },
                        "metadata": { "type": "object" },
                        "permissions": { "type": "object" },
                        "auto_rotate": { "type": "boolean", "nullable": true },
                        "rotation_interval": { "type": "string", "nullable": true },
                        "created_at": { "type": "string", "format": "date-time", "nullable": true },
                        "updated_at": { "type": "string", "format": "date-time", "nullable": true }
                    }
                },
                "KeyListResponse": {
                    "type": "object",
                    "properties": {
                        "keys": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "token": { "type": "string" },
                                    "key_name": { "type": "string", "nullable": true },
                                    "key_alias": { "type": "string", "nullable": true },
                                    "user_id": { "type": "string", "nullable": true },
                                    "team_id": { "type": "string", "nullable": true },
                                    "spend": { "type": "number", "format": "float" },
                                    "max_budget": { "type": "number", "format": "float", "nullable": true },
                                    "tpm_limit": { "type": "integer", "format": "int64", "nullable": true },
                                    "rpm_limit": { "type": "integer", "format": "int64", "nullable": true },
                                    "blocked": { "type": "boolean", "nullable": true },
                                    "expires": { "type": "string", "format": "date-time", "nullable": true },
                                    "models": { "type": "array", "items": { "type": "string" } },
                                    "metadata": { "type": "object" },
                                    "created_at": { "type": "string", "format": "date-time", "nullable": true }
                                }
                            }
                        }
                    }
                },
                "KeyUpdateRequest": {
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": { "type": "string", "description": "Existing key value or key_alias to identify the key" },
                        "key_alias": { "type": "string" },
                        "key_name": { "type": "string" },
                        "user_id": { "type": "string" },
                        "team_id": { "type": "string" },
                        "max_budget": { "type": "number", "format": "float" },
                        "tpm_limit": { "type": "integer", "format": "int64" },
                        "rpm_limit": { "type": "integer", "format": "int64" },
                        "blocked": { "type": "boolean" },
                        "models": { "type": "array", "items": { "type": "string" } },
                        "metadata": { "type": "object" },
                        "expires": { "type": "string", "format": "date-time" }
                    }
                },
                "KeyRegenerateRequest": {
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": { "type": "string", "description": "Existing key to regenerate" },
                        "key_alias": { "type": "string", "description": "New alias for the regenerated key" },
                        "new_expiry": { "type": "string", "format": "date-time" }
                    }
                },
                "SpendLogsResponse": {
                    "type": "object",
                    "properties": {
                        "data": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "request_id": { "type": "string" },
                                    "call_type": { "type": "string" },
                                    "api_key": { "type": "string" },
                                    "spend": { "type": "number", "format": "float" },
                                    "total_tokens": { "type": "integer" },
                                    "prompt_tokens": { "type": "integer" },
                                    "completion_tokens": { "type": "integer" },
                                    "start_time": { "type": "string", "format": "date-time" },
                                    "end_time": { "type": "string", "format": "date-time" },
                                    "model": { "type": "string" },
                                    "user": { "type": "string", "nullable": true },
                                    "request_tags": { "type": "array", "nullable": true },
                                    "status": { "type": "string", "nullable": true }
                                }
                            }
                        },
                        "count": { "type": "integer" }
                    }
                },
                "SpendPerKeyResponse": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string" },
                        "spend": { "type": "number", "format": "float" }
                    }
                },
                "SpendPerUserResponse": {
                    "type": "object",
                    "properties": {
                        "user_id": { "type": "string" },
                        "spend": { "type": "number", "format": "float" }
                    }
                },
                "SpendPerTagResponse": {
                    "type": "object",
                    "properties": {
                        "tag": { "type": "string" },
                        "spend": { "type": "number", "format": "float" }
                    }
                },
                "GlobalSpendResponse": {
                    "type": "object",
                    "properties": {
                        "spend": { "type": "number", "format": "float" }
                    }
                },
                "GlobalSpendKeysResponse": {
                    "type": "object",
                    "properties": {
                        "data": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "api_key": { "type": "string" },
                                    "spend": { "type": "number", "format": "float" }
                                }
                            }
                        }
                    }
                },
                "ChatCompletionRequest": {
                    "type": "object",
                    "required": ["model", "messages"],
                    "properties": {
                        "model": { "type": "string", "description": "Model ID (e.g. gpt-4, gpt-3.5-turbo)" },
                        "messages": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["role", "content"],
                                "properties": {
                                    "role": { "type": "string", "enum": ["system", "user", "assistant"] },
                                    "content": { "description": "Message content — string or array of content parts" },
                                    "name": { "type": "string" }
                                }
                            }
                        },
                        "stream": { "type": "boolean", "default": false, "description": "Enable SSE streaming" },
                        "max_tokens": { "type": "integer" },
                        "temperature": { "type": "number", "format": "float" },
                        "top_p": { "type": "number", "format": "float" },
                        "frequency_penalty": { "type": "number", "format": "float" },
                        "presence_penalty": { "type": "number", "format": "float" },
                        "stop": { "type": "array", "items": { "type": "string" } },
                        "user": { "type": "string" }
                    }
                },
                "ModelsListResponse": {
                    "type": "object",
                    "properties": {
                        "object": { "type": "string", "enum": ["list"] },
                        "data": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "object": { "type": "string" },
                                    "created": { "type": "integer", "format": "int64" },
                                    "owned_by": { "type": "string" }
                                }
                            }
                        }
                    }
                },
                "HealthResponse": {
                    "type": "object",
                    "properties": {
                        "status": { "type": "string" }
                    }
                }
            }
        },
        "security": [
            { "BearerAuth": [] }
        ]
    })
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Path item builders
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn auth_ref() -> Value {
    json!([{ "BearerAuth": [] }])
}

fn common_responses() -> Value {
    json!({
        "400": {
            "description": "Bad request — missing or invalid parameters",
            "content": {
                "application/json": {
                    "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                }
            }
        },
        "401": {
            "description": "Unauthorized — missing or invalid authentication",
            "content": {
                "application/json": {
                    "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                }
            }
        },
        "500": {
            "description": "Internal server error",
            "content": {
                "application/json": {
                    "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                }
            }
        }
    })
}

fn key_generate_spec() -> Value {
    json!({
        "tags": ["Key Management"],
        "summary": "Generate a new virtual API key",
        "description": "Creates a new virtual API key with optional model restrictions, budget limits, and rate limits. Returns the raw key value (shown once).",
        "operationId": "generateKey",
        "security": auth_ref(),
        "requestBody": {
            "required": true,
            "content": {
                "application/json": {
                    "schema": { "$ref": "#/components/schemas/GenerateKeyRequest" }
                }
            }
        },
        "responses": {
            "200": {
                "description": "Key generated successfully",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/KeyResponse" }
                    }
                }
            },
            "409": {
                "description": "Conflict — key already exists",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "400": common_responses()["400"],
            "401": common_responses()["401"],
            "500": common_responses()["500"]
        }
    })
}

fn key_info_spec() -> Value {
    json!({
        "tags": ["Key Management"],
        "summary": "Get key information",
        "description": "Retrieve information about a specific API key by its raw value or alias.",
        "operationId": "getKeyInfo",
        "security": auth_ref(),
        "parameters": [
            {
                "name": "key",
                "in": "query",
                "required": false,
                "description": "Raw API key value",
                "schema": { "type": "string" }
            },
            {
                "name": "key_alias",
                "in": "query",
                "required": false,
                "description": "Key alias (alternative to key)",
                "schema": { "type": "string" }
            }
        ],
        "responses": {
            "200": {
                "description": "Key information retrieved",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/KeyResponse" }
                    }
                }
            },
            "404": {
                "description": "Key not found",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "400": common_responses()["400"],
            "500": common_responses()["500"]
        }
    })
}

fn key_list_spec() -> Value {
    json!({
        "tags": ["Key Management"],
        "summary": "List all keys",
        "description": "List all virtual API keys, optionally filtered by team or user.",
        "operationId": "listKeys",
        "security": auth_ref(),
        "parameters": [
            {
                "name": "team_id",
                "in": "query",
                "required": false,
                "description": "Filter by team ID",
                "schema": { "type": "string" }
            },
            {
                "name": "user_id",
                "in": "query",
                "required": false,
                "description": "Filter by user ID",
                "schema": { "type": "string" }
            }
        ],
        "responses": {
            "200": {
                "description": "List of keys",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/KeyListResponse" }
                    }
                }
            },
            "500": common_responses()["500"]
        }
    })
}

fn key_update_spec() -> Value {
    json!({
        "tags": ["Key Management"],
        "summary": "Update a key",
        "description": "Update an existing virtual API key's configuration.",
        "operationId": "updateKey",
        "security": auth_ref(),
        "requestBody": {
            "required": true,
            "content": {
                "application/json": {
                    "schema": { "$ref": "#/components/schemas/KeyUpdateRequest" }
                }
            }
        },
        "responses": {
            "200": {
                "description": "Key updated successfully",
                "content": {
                    "application/json": {
                        "schema": {
                            "type": "object",
                            "properties": {
                                "status": { "type": "string", "example": "ok" },
                                "message": { "type": "string" }
                            }
                        }
                    }
                }
            },
            "400": common_responses()["400"],
            "401": common_responses()["401"],
            "404": {
                "description": "Key not found",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "500": common_responses()["500"]
        }
    })
}

fn key_delete_spec() -> Value {
    json!({
        "tags": ["Key Management"],
        "summary": "Delete a key",
        "description": "Delete (soft-delete) a virtual API key.",
        "operationId": "deleteKey",
        "security": auth_ref(),
        "parameters": [
            {
                "name": "key",
                "in": "query",
                "required": false,
                "description": "Raw API key value to delete",
                "schema": { "type": "string" }
            },
            {
                "name": "key_aliases",
                "in": "query",
                "required": false,
                "description": "Key alias to delete (alternative to key)",
                "schema": { "type": "string" }
            }
        ],
        "responses": {
            "200": {
                "description": "Key deleted successfully",
                "content": {
                    "application/json": {
                        "schema": {
                            "type": "object",
                            "properties": {
                                "status": { "type": "string", "example": "ok" },
                                "message": { "type": "string" }
                            }
                        }
                    }
                }
            },
            "400": common_responses()["400"],
            "401": common_responses()["401"],
            "404": {
                "description": "Key not found",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "500": common_responses()["500"]
        }
    })
}

fn key_regenerate_spec() -> Value {
    json!({
        "tags": ["Key Management"],
        "summary": "Regenerate a key",
        "description": "Regenerate a virtual API key — creates a new key value while copying the existing key's configuration, then deletes the old key.",
        "operationId": "regenerateKey",
        "security": auth_ref(),
        "requestBody": {
            "required": true,
            "content": {
                "application/json": {
                    "schema": { "$ref": "#/components/schemas/KeyRegenerateRequest" }
                }
            }
        },
        "responses": {
            "200": {
                "description": "Key regenerated successfully",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/KeyResponse" }
                    }
                }
            },
            "400": common_responses()["400"],
            "401": common_responses()["401"],
            "404": {
                "description": "Original key not found",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "500": common_responses()["500"]
        }
    })
}

fn spend_logs_spec() -> Value {
    json!({
        "tags": ["Spend & Usage"],
        "summary": "Query spend logs",
        "description": "Retrieve spend logs for the authenticated key.",
        "operationId": "getSpendLogs",
        "security": auth_ref(),
        "parameters": [
            {
                "name": "api_key",
                "in": "query",
                "required": false,
                "description": "Filter logs by API key hash",
                "schema": { "type": "string" }
            },
            {
                "name": "limit",
                "in": "query",
                "required": false,
                "description": "Maximum number of logs to return",
                "schema": { "type": "integer" }
            }
        ],
        "responses": {
            "200": {
                "description": "Spend logs retrieved",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/SpendLogsResponse" }
                    }
                }
            },
            "401": common_responses()["401"],
            "500": common_responses()["500"]
        }
    })
}

fn spend_keys_spec() -> Value {
    json!({
        "tags": ["Spend & Usage"],
        "summary": "Spend per key",
        "description": "Get total spend for the authenticated key.",
        "operationId": "getSpendPerKey",
        "security": auth_ref(),
        "responses": {
            "200": {
                "description": "Spend per key",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/SpendPerKeyResponse" }
                    }
                }
            },
            "401": common_responses()["401"],
            "500": common_responses()["500"]
        }
    })
}

fn spend_users_spec() -> Value {
    json!({
        "tags": ["Spend & Usage"],
        "summary": "Spend per user",
        "description": "Get total spend for the user associated with the authenticated key.",
        "operationId": "getSpendPerUser",
        "security": auth_ref(),
        "responses": {
            "200": {
                "description": "Spend per user",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/SpendPerUserResponse" }
                    }
                }
            },
            "401": common_responses()["401"],
            "500": common_responses()["500"]
        }
    })
}

fn spend_tags_spec() -> Value {
    json!({
        "tags": ["Spend & Usage"],
        "summary": "Spend per tag",
        "description": "Get total spend for a specific request tag.",
        "operationId": "getSpendPerTag",
        "security": auth_ref(),
        "parameters": [
            {
                "name": "tag",
                "in": "query",
                "required": true,
                "description": "Tag to query spend for",
                "schema": { "type": "string" }
            }
        ],
        "responses": {
            "200": {
                "description": "Spend per tag",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/SpendPerTagResponse" }
                    }
                }
            },
            "400": common_responses()["400"],
            "401": common_responses()["401"],
            "500": common_responses()["500"]
        }
    })
}

fn global_spend_spec() -> Value {
    json!({
        "tags": ["Spend & Usage"],
        "summary": "Global spend",
        "description": "Get total global spend across all keys. Requires admin (master key) authentication.",
        "operationId": "getGlobalSpend",
        "security": auth_ref(),
        "responses": {
            "200": {
                "description": "Global spend total",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/GlobalSpendResponse" }
                    }
                }
            },
            "401": common_responses()["401"],
            "403": {
                "description": "Forbidden — admin access required",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "500": common_responses()["500"]
        }
    })
}

fn global_spend_logs_spec() -> Value {
    json!({
        "tags": ["Spend & Usage"],
        "summary": "All spend logs (admin)",
        "description": "Retrieve all spend logs across all keys. Requires admin (master key) authentication.",
        "operationId": "getGlobalSpendLogs",
        "security": auth_ref(),
        "parameters": [
            {
                "name": "api_key",
                "in": "query",
                "required": false,
                "description": "Filter logs by API key hash",
                "schema": { "type": "string" }
            },
            {
                "name": "limit",
                "in": "query",
                "required": false,
                "description": "Maximum number of logs to return",
                "schema": { "type": "integer" }
            }
        ],
        "responses": {
            "200": {
                "description": "All spend logs",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/SpendLogsResponse" }
                    }
                }
            },
            "401": common_responses()["401"],
            "403": {
                "description": "Forbidden — admin access required",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "500": common_responses()["500"]
        }
    })
}

fn global_spend_keys_spec() -> Value {
    json!({
        "tags": ["Spend & Usage"],
        "summary": "All keys spend (admin)",
        "description": "Get spend summary for all keys. Requires admin (master key) authentication.",
        "operationId": "getGlobalSpendKeys",
        "security": auth_ref(),
        "responses": {
            "200": {
                "description": "Spend per key for all keys",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/GlobalSpendKeysResponse" }
                    }
                }
            },
            "401": common_responses()["401"],
            "403": {
                "description": "Forbidden — admin access required",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "500": common_responses()["500"]
        }
    })
}

fn chat_completions_spec() -> Value {
    json!({
        "tags": ["Chat & Models"],
        "summary": "Chat completions",
        "description": "OpenAI-compatible chat completions endpoint. Supports both streaming (SSE) and non-streaming responses. Validates the API key, checks model permissions and budget, then proxies to the upstream LLM provider.",
        "operationId": "createChatCompletion",
        "security": auth_ref(),
        "requestBody": {
            "required": true,
            "content": {
                "application/json": {
                    "schema": { "$ref": "#/components/schemas/ChatCompletionRequest" }
                }
            }
        },
        "responses": {
            "200": {
                "description": "Chat completion response (non-streaming JSON or SSE stream)",
                "content": {
                    "application/json": {
                        "schema": {
                            "type": "object",
                            "description": "OpenAI-compatible chat completion response. For streaming, returns text/event-stream."
                        }
                    }
                }
            },
            "400": common_responses()["400"],
            "401": common_responses()["401"],
            "403": {
                "description": "Forbidden — model not allowed or budget exceeded",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "429": {
                "description": "Too many requests — rate limit or budget exceeded",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "502": {
                "description": "Bad gateway — upstream provider error",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ErrorResponse" }
                    }
                }
            },
            "500": common_responses()["500"]
        }
    })
}

fn models_list_spec() -> Value {
    json!({
        "tags": ["Chat & Models"],
        "summary": "List available models",
        "description": "Returns the list of models the authenticated key has access to. The master key sees all available models.",
        "operationId": "listModels",
        "security": auth_ref(),
        "responses": {
            "200": {
                "description": "Model list",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/ModelsListResponse" }
                    }
                }
            },
            "401": common_responses()["401"],
            "500": common_responses()["500"]
        }
    })
}

fn health_spec() -> Value {
    json!({
        "tags": ["Health"],
        "summary": "Health check",
        "description": "Basic health check endpoint. Returns 200 if the server is running.",
        "operationId": "healthCheck",
        "security": [],
        "responses": {
            "200": {
                "description": "Server is healthy",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/HealthResponse" }
                    }
                }
            }
        }
    })
}

fn health_readiness_spec() -> Value {
    json!({
        "tags": ["Health"],
        "summary": "Readiness check",
        "description": "Checks if the server is ready to accept traffic. Returns 200 when ready.",
        "operationId": "healthReadiness",
        "security": [],
        "responses": {
            "200": {
                "description": "Server is ready",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/HealthResponse" }
                    }
                }
            }
        }
    })
}

fn health_liveliness_spec() -> Value {
    json!({
        "tags": ["Health"],
        "summary": "Liveliness check",
        "description": "Checks if the server is alive. Returns 200 when alive.",
        "operationId": "healthLiveliness",
        "security": [],
        "responses": {
            "200": {
                "description": "Server is alive",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/HealthResponse" }
                    }
                }
            }
        }
    })
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Axum handler for /openapi.json
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// GET /openapi.json — Serve the generated OpenAPI 3.1 specification.
///
/// ```no_compile
/// .route("/openapi.json", axum::routing::get(openapi_json))
/// ```
pub async fn openapi_json() -> axum::Json<Value> {
    axum::Json(generate_openapi_spec())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
        Router,
    };
    use tower::util::ServiceExt;

    #[test]
    fn test_openapi_spec_valid() {
        let spec = generate_openapi_spec();

        // Verify OpenAPI version
        assert_eq!(spec.get("openapi").and_then(|v| v.as_str()), Some("3.1.0"));

        // Verify info section
        let info = spec.get("info").expect("info section must exist");
        assert!(info.get("title").and_then(|v| v.as_str()).is_some());
        assert!(info.get("version").and_then(|v| v.as_str()).is_some());

        // Verify paths exist
        let paths = spec.get("paths").expect("paths must exist");
        assert!(paths.is_object());

        // Verify components exist with securitySchemes
        let components = spec.get("components").expect("components must exist");
        let security_schemes = components
            .get("securitySchemes")
            .expect("securitySchemes must exist");
        assert!(security_schemes.get("BearerAuth").is_some());
    }

    #[test]
    fn test_openapi_has_all_endpoints() {
        let spec = generate_openapi_spec();
        let paths = spec
            .get("paths")
            .expect("paths must exist")
            .as_object()
            .unwrap();

        let expected_endpoints = vec![
            "/key/generate",
            "/key/info",
            "/key/list",
            "/key/update",
            "/key/delete",
            "/key/regenerate",
            "/spend/logs",
            "/spend/keys",
            "/spend/users",
            "/spend/tags",
            "/global/spend",
            "/global/spend/logs",
            "/global/spend/keys",
            "/v1/chat/completions",
            "/v1/models",
            "/health",
            "/health/readiness",
            "/health/liveliness",
        ];

        assert_eq!(
            paths.len(),
            expected_endpoints.len(),
            "Expected {} paths, got {}",
            expected_endpoints.len(),
            paths.len()
        );

        for endpoint in &expected_endpoints {
            assert!(paths.contains_key(*endpoint), "Missing path: {}", endpoint);
        }
    }

    #[test]
    fn test_openapi_tags() {
        let spec = generate_openapi_spec();
        let tags = spec
            .get("tags")
            .expect("tags must exist")
            .as_array()
            .unwrap();

        let tag_names: Vec<&str> = tags
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .collect();

        assert!(tag_names.contains(&"Key Management"));
        assert!(tag_names.contains(&"Spend & Usage"));
        assert!(tag_names.contains(&"Chat & Models"));
        assert!(tag_names.contains(&"Health"));
    }

    #[test]
    fn test_openapi_components_schemas() {
        let spec = generate_openapi_spec();
        let schemas = spec
            .get("components")
            .and_then(|c| c.get("schemas"))
            .expect("schemas must exist")
            .as_object()
            .unwrap();

        let expected_schemas = vec![
            "ErrorResponse",
            "GenerateKeyRequest",
            "KeyResponse",
            "KeyListResponse",
            "KeyUpdateRequest",
            "KeyRegenerateRequest",
            "SpendLogsResponse",
            "SpendPerKeyResponse",
            "SpendPerUserResponse",
            "SpendPerTagResponse",
            "GlobalSpendResponse",
            "GlobalSpendKeysResponse",
            "ChatCompletionRequest",
            "ModelsListResponse",
            "HealthResponse",
        ];

        for schema_name in &expected_schemas {
            assert!(
                schemas.contains_key(*schema_name),
                "Missing schema: {}",
                schema_name
            );
        }
    }

    #[test]
    fn test_openapi_health_no_auth() {
        let spec = generate_openapi_spec();
        let health = spec
            .pointer("/paths/~1health/get")
            .expect("health path must exist");

        // Health endpoints should have empty security (no auth required)
        let security = health.get("security").expect("security must exist");
        assert!(
            security.as_array().map(|a| a.is_empty()).unwrap_or(false),
            "Health endpoints should NOT require authentication"
        );
    }

    #[test]
    fn test_openapi_chat_requires_auth() {
        let spec = generate_openapi_spec();
        let chat = spec
            .pointer("/paths/~1v1~1chat~1completions/post")
            .expect("chat path must exist");

        let security = chat.get("security").and_then(|s| s.as_array()).unwrap();
        assert!(
            !security.is_empty(),
            "Chat endpoint should require authentication"
        );
    }

    #[tokio::test]
    async fn test_openapi_json_endpoint() {
        // Build a minimal app with the openapi route
        let app = Router::new().route("/openapi.json", axum::routing::get(openapi_json));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/openapi.json")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let spec: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Verify it's valid OpenAPI
        assert_eq!(spec.get("openapi").and_then(|v| v.as_str()), Some("3.1.0"));
        assert!(spec.get("info").is_some());
        assert!(spec.get("paths").is_some());
    }

    #[tokio::test]
    async fn test_openapi_json_content_type() {
        let app = Router::new().route("/openapi.json", axum::routing::get(openapi_json));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/openapi.json")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok());

        assert!(
            content_type
                .map(|ct| ct.starts_with("application/json"))
                .unwrap_or(false),
            "Expected application/json content-type, got: {:?}",
            content_type
        );
    }
}
