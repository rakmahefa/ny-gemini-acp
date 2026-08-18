[
  {
    "_direction": "outgoing",
    "_type": "request",
    "id": "b8dfb36c-8598-4fc6-9d1d-f07bb7e8579e",
    "method": "initialize",
    "params": {
      "protocolVersion": 1,
      "clientCapabilities": {
        "fs": {
          "readTextFile": true,
          "writeTextFile": true
        },
        "terminal": true,
        "session": {
          "configOptions": {
            "boolean": {}
          }
        },
        "auth": {
          "terminal": true
        },
        "elicitation": {
          "form": {},
          "url": {}
        },
        "_meta": {
          "terminal_output": true,
          "terminal-auth": true
        }
      },
      "clientInfo": {
        "name": "zed",
        "title": "Zed",
        "version": "1.13.2+stable"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "response",
    "id": "b8dfb36c-8598-4fc6-9d1d-f07bb7e8579e",
    "method": "initialize",
    "params": {
      "protocolVersion": 1,
      "agentCapabilities": {
        "loadSession": true,
        "promptCapabilities": {
          "image": false,
          "audio": false,
          "embeddedContext": false
        },
        "mcpCapabilities": {
          "http": false,
          "sse": false
        },
        "sessionCapabilities": {
          "list": {},
          "delete": {},
          "fork": {},
          "resume": {},
          "close": {}
        },
        "auth": {}
      },
      "authMethods": [],
      "agentInfo": {
        "name": "gemini-acp",
        "title": "Gemini (Web)",
        "version": "0.2.2"
      }
    }
  },
  {
    "_direction": "outgoing",
    "_type": "request",
    "id": "2722fbdb-8a42-4e7c-a110-013a2ca9b83a",
    "method": "session/load",
    "params": {
      "mcpServers": [
        {
          "name": "mcp-libre",
          "command": "uv",
          "args": [
            "run",
            "--directory",
            "/home/neko/Documents/GD-IGP/mcp-libre",
            "python",
            "src/main.py"
          ],
          "env": []
        }
      ],
      "cwd": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace",
      "sessionId": "sess_968d0d25a7334130a494b831515e1797"
    }
  },
  {
    "_direction": "outgoing",
    "_type": "request",
    "id": "81aa4385-ed95-406e-8b58-44c8613ce114",
    "method": "session/new",
    "params": {
      "cwd": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace",
      "mcpServers": [
        {
          "name": "mcp-libre",
          "command": "uv",
          "args": [
            "run",
            "--directory",
            "/home/neko/Documents/GD-IGP/mcp-libre",
            "python",
            "src/main.py"
          ],
          "env": []
        }
      ]
    }
  },
  {
    "_direction": "stderr",
    "_type": "stderr",
    "id": null,
    "method": "stderr",
    "params": "\u001b[2m2026-08-18T05:50:31.398013Z\u001b[0m \u001b[33m WARN\u001b[0m \u001b[1mconnection\u001b[0m\u001b[1m{\u001b[0m\u001b[3mname\u001b[0m\u001b[2m=\u001b[0m\"gemini-acp\"\u001b[1m}\u001b[0m\u001b[2m:\u001b[0m \u001b[2mgemini_acp_agent::handlers::session\u001b[0m\u001b[2m:\u001b[0m session/new received mcp_servers, but Gemini ACP does not wire them yet \u001b[3mcount\u001b[0m\u001b[2m=\u001b[0m1"
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "session_info_update",
        "title": "Réponds exactement : Bonjour"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Réponds exactement : Bonjour"
        },
        "messageId": "msg_0"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Bonjour"
        },
        "messageId": "msg_1"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Réponds avec 5 lignes numérotées."
        },
        "messageId": "msg_2"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "1. Ligne 1\n2. Ligne 2\n3. Ligne 3\n4. Ligne 4\n5. Ligne 5"
        },
        "messageId": "msg_3"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Réponds avec du Markdown et un bloc ```rust ... ```."
        },
        "messageId": "msg_4"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Voici un exemple de code Rust basique :\n\n```rust\nfn main() {\n    println!(\"Hello, World!\");\n}\n```"
        },
        "messageId": "msg_5"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Dis-moi : \"bonjour\", 'salut', …, é, ç, 日本語"
        },
        "messageId": "msg_6"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "\"bonjour\", 'salut', …, é, ç, 日本語"
        },
        "messageId": "msg_7"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Ne fais aucun outil, réponds simplement OK."
        },
        "messageId": "msg_8"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "OK"
        },
        "messageId": "msg_9"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Liste les fichiers du projet."
        },
        "messageId": "msg_10"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Je vais lister les fichiers présents dans le répertoire de travail actuel pour vous montrer le contenu du projet."
        },
        "messageId": "msg_11"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_0",
        "title": "Find paths `**/*`",
        "kind": "search",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**🧭 Glob**\n⏳ pending  ·  🔓 no permission  ·  ✅ low\n\n**Input**\n```text\npattern `**/*`  ·  path .  ·  max 100\n```"
            }
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/."
          }
        ],
        "rawInput": {
          "pattern": "**/*"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call_update",
        "toolCallId": "replay_call_0",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**🧭 Glob**\n🟢 completed  ·  🔓 no permission  ·  ✅ low\n\n**Output**\n```text\n[Tool result]: {\"tool\":\"glob\",\"content\":\"Aucun chemin correspondant.\"}\n```"
            }
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/[Tool result]: {\"tool\":\"glob\",\"content\":\"Aucun chemin correspondant.\"}"
          }
        ]
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Je vais vérifier le contenu du répertoire courant en listant les fichiers et dossiers avec la commande `shell_exec`."
        },
        "messageId": "msg_13"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_1",
        "title": "ls -la",
        "kind": "execute",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**▣ Shell**\n⏳ pending  ·  🔐 permission  ·  ✅ low\n\n**Input**\n```text\nls -la\n```"
            }
          }
        ],
        "rawInput": {
          "command": "ls -la"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call_update",
        "toolCallId": "replay_call_1",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**▣ Shell**\n🟢 completed  ·  🔐 permission  ·  ✅ low\n\n**Output**\n```text\n[Tool result]: {\"tool\":\"shell_exec\",\"content\":\"total 8\\ndrwxr-xr-x 2 neko neko 4096 Aog 17 23:42 .\\ndrwxr-xr-x 7 neko neko 4096 Aog 17 14:14 ..\\n\\n\"}\n```"
            }
          }
        ],
        "locations": []
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Le répertoire courant (`/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace`) est totalement vide. Aucun fichier ni dossier n'est présent pour le moment."
        },
        "messageId": "msg_15"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "cree un fichier qui contenant du Markdown, des quotes et des ``` si possible"
        },
        "messageId": "msg_16"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Je vais créer un fichier Markdown nommé `example.md` contenant du Markdown standard, des guillemets/quotes, et des blocs de code avec triples backticks."
        },
        "messageId": "msg_17"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_2",
        "title": "Write example.md",
        "kind": "edit",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**📝 File Write**\n⏳ pending  ·  🔐 permission  ·  ⚠️ medium\n\n**Input**\n```text\nexample.md  ·  677 chars\n```"
            }
          },
          {
            "type": "diff",
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/example.md",
            "oldText": "# Exemple de document Markdown\n\nVoici un exemple démontrant l'utilisation de **Markdown**, de *citations* (quotes) et de blocs de code.\n\n## Citation (Quote)\n\n> \"La simplicité est la sophistication suprême.\"\n> — Leonardo da Vinci\n\n## Exemples de guillemets\n\n- Guillemets simples : 'exemple'\n- Guillemets doubles : \"exemple\"\n- Guillemets français : « exemple »\n\n## Blocs de code (Code Blocks)\n\nVoici un exemple de code Python :\n\n```python\ndef saluer(nom: str) -> None:\n    \"\"\"Affiche un message de salutation.\"\"\"\n    print(f\"Bonjour, {nom} !\")\n\nif __name__ == \"__main__\":\n    saluer(\"Monde\")\n```\n\nEt un exemple en Rust :\n\n```rust\nfn main() {\n    println!(\"Hello, World!\");\n}\n```\n",
            "newText": "# Exemple de document Markdown\n\nVoici un exemple démontrant l'utilisation de **Markdown**, de *citations* (quotes) et de blocs de code.\n\n## Citation (Quote)\n\n> \"La simplicité est la sophistication suprême.\"\n> — Leonardo da Vinci\n\n## Exemples de guillemets\n\n- Guillemets simples : 'exemple'\n- Guillemets doubles : \"exemple\"\n- Guillemets français : « exemple »\n\n## Blocs de code (Code Blocks)\n\nVoici un exemple de code Python :\n\n```python\ndef saluer(nom: str) -> None:\n    \"\"\"Affiche un message de salutation.\"\"\"\n    print(f\"Bonjour, {nom} !\")\n\nif __name__ == \"__main__\":\n    saluer(\"Monde\")\n```\n\nEt un exemple en Rust :\n\n```rust\nfn main() {\n    println!(\"Hello, World!\");\n}\n```\n"
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/example.md"
          }
        ],
        "rawInput": {
          "content": "# Exemple de document Markdown\n\nVoici un exemple démontrant l'utilisation de **Markdown**, de *citations* (quotes) et de blocs de code.\n\n## Citation (Quote)\n\n> \"La simplicité est la sophistication suprême.\"\n> — Leonardo da Vinci\n\n## Exemples de guillemets\n\n- Guillemets simples : 'exemple'\n- Guillemets doubles : \"exemple\"\n- Guillemets français : « exemple »\n\n## Blocs de code (Code Blocks)\n\nVoici un exemple de code Python :\n\n```python\ndef saluer(nom: str) -> None:\n    \"\"\"Affiche un message de salutation.\"\"\"\n    print(f\"Bonjour, {nom} !\")\n\nif __name__ == \"__main__\":\n    saluer(\"Monde\")\n```\n\nEt un exemple en Rust :\n\n```rust\nfn main() {\n    println!(\"Hello, World!\");\n}\n```\n",
          "path": "example.md"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call_update",
        "toolCallId": "replay_call_2",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**📝 File Write**\n🟢 completed  ·  🔐 permission  ·  ⚠️ medium\n\n**Output**\n```text\n[Tool result]: {\"tool\":\"file_write\",\"content\":\"Fichier écrit: /run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/example.md\"}\n```"
            }
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/example.md"
          }
        ]
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Le fichier `example.md` a été créé avec succès.\n\n**Fichier créé :** `example.md`\n**Contenu :**\n```markdown\n# Exemple de document Markdown\n\nVoici un exemple démontrant l'utilisation de **Markdown**, de *citations* (quotes) et de blocs de code.\n\n## Citation (Quote)\n\n> \"La simplicité est la sophistication suprême.\"\n> — Leonardo da Vinci\n\n## Exemples de guillemets\n\n- Guillemets simples : 'exemple'\n- Guillemets doubles : \"exemple\"\n- Guillemets français : « exemple »\n\n## Blocs de code (Code Blocks)\n\nVoici un exemple de code Python :\n\n```python\ndef saluer(nom: str) -> None:\n    \"\"\"Affiche un message de salutation.\"\"\"\n    print(f\"Bonjour, {nom} !\")\n\nif __name__ == \"__main__\":\n    saluer(\"Monde\")\n```\n\nEt un exemple en Rust :\n\n```rust\nfn main() {\n    println!(\"Hello, World!\");\n}\n```\n```"
        },
        "messageId": "msg_19"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "lis un fichier contenant du Markdown, des quotes et des ``` si possible"
        },
        "messageId": "msg_20"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Je vais lire le fichier `example.md` que nous venons de créer pour afficher son contenu exact."
        },
        "messageId": "msg_21"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_3",
        "title": "Read example.md (1-500)",
        "kind": "read",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**📖 File Read**\n⏳ pending  ·  🔓 no permission  ·  ✅ low\n\n**Input**\n```text\nexample.md  ·  lignes 1-500\n```"
            }
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/example.md",
            "line": 1
          }
        ],
        "rawInput": {
          "limit": 500,
          "offset": 1,
          "path": "example.md"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call_update",
        "toolCallId": "replay_call_3",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**📖 File Read**\n🟢 completed  ·  🔓 no permission  ·  ✅ low\n\n**Output**\n```text\n1\t[Tool result]: {\"tool\":\"file_read\",\"content\":\"# Exemple de document Markdown\\n\\nVoici un exemple démontrant l'utilisation de **Markdown**, de *citations* (quotes) et de blocs de code.\\n\\n## Citation (Quote)\\n\\n> \\\"La simplicité est la sophistication suprême.\\\"\\n> — Leonardo da Vinci\\n\\n## Exemples de guillemets\\n\\n- Guillemets simples : 'exemple'\\n- Guillemets doubles : \\\"exemple\\\"\\n- Guillemets français : « exemple »\\n\\n## Blocs de code (Code Blocks)\\n\\nVoici un exemple de code Python :\\n\\n```python\\ndef saluer(nom: str) -> None:\\n    \\\"\\\"\\\"Affiche un message de salutation.\\\"\\\"\\\"\\n    print(f\\\"Bonjour, {nom} !\\\")\\n\\nif __name__ == \\\"__main__\\\":\\n    saluer(\\\"Monde\\\")\\n```\\n\\nEt un exemple en Rust :\\n\\n```rust\\nfn main() {\\n    println!(\\\"Hello, World!\\\");\\n}\\n```\"}\n```"
            }
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/example.md"
          }
        ]
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Voici le contenu lu depuis le fichier `example.md` (lignes 1 à 33) :\n\n```markdown\n# Exemple de document Markdown\n\nVoici un exemple démontrant l'utilisation de **Markdown**, de *citations* (quotes) et de blocs de code.\n\n## Citation (Quote)\n\n> \"La simplicité est la sophistication suprême.\"\n> — Leonardo da Vinci\n\n## Exemples de guillemets\n\n- Guillemets simples : 'exemple'\n- Guillemets doubles : \"exemple\"\n- Guillemets français : « exemple »\n\n## Blocs de code (Code Blocks)\n\nVoici un exemple de code Python :\n\n```python\ndef saluer(nom: str) -> None:\n    \"\"\"Affiche un message de salutation.\"\"\"\n    print(f\"Bonjour, {nom} !\")\n\nif __name__ == \"__main__\":\n    saluer(\"Monde\")\n```\n\nEt un exemple en Rust :\n\n```rust\nfn main() {\n    println!(\"Hello, World!\");\n}\n```\n```"
        },
        "messageId": "msg_23"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_4",
        "title": "Follow-up · Ajouter un script d'exécution pour tester les extraits de code",
        "status": "in_progress",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**↪ Follow-up**\n⏳ pending  ·  🔓 no permission  ·  ✅ low\n\n**Content**\n```text\nAjouter un script d'exécution pour tester les extraits de code\n→ Ajoute un script Python ou Shell pour exécuter le code présent dans example.md\n```"
            }
          }
        ],
        "rawInput": {
          "label": "Ajouter un script d'exécution pour tester les extraits de code",
          "query": "Ajoute un script Python ou Shell pour exécuter le code présent dans example.md"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_5",
        "title": "Follow-up · Ajouter un script d'exécution pour tester les extraits de code",
        "status": "in_progress",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**↪ Follow-up**\n⏳ pending  ·  🔓 no permission  ·  ✅ low\n\n**Content**\n```text\nAjouter un script d'exécution pour tester les extraits de code\n→ Ajoute un script Python ou Shell pour exécuter le code présent dans example.md\n```"
            }
          }
        ],
        "rawInput": {
          "label": "Ajouter un script d'exécution pour tester les extraits de code",
          "query": "Ajoute un script Python ou Shell pour exécuter le code présent dans example.md"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Voici le contenu lu depuis le fichier `example.md` (lignes 1 à 33) :\n\n```markdown\n# Exemple de document Markdown\n\nVoici un exemple démontrant l'utilisation de **Markdown**, de *citations* (quotes) et de blocs de code.\n\n## Citation (Quote)\n\n> \"La simplicité est la sophistication suprême.\"\n> — Leonardo da Vinci\n\n## Exemples de guillemets\n\n- Guillemets simples : 'exemple'\n- Guillemets doubles : \"exemple\"\n- Guillemets français : « exemple »\n\n## Blocs de code (Code Blocks)\n\nVoici un exemple de code Python :\n\n```python\ndef saluer(nom: str) -> None:\n    \"\"\"Affiche un message de salutation.\"\"\"\n    print(f\"Bonjour, {nom} !\")\n\nif __name__ == \"__main__\":\n    saluer(\"Monde\")\n```\n\nEt un exemple en Rust :\n\n```rust\nfn main() {\n    println!(\"Hello, World!\");\n}\n```\n```"
        },
        "messageId": "msg_24"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_6",
        "title": "Follow-up · Ajouter un script d'exécution pour tester les extraits de code",
        "status": "in_progress",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**↪ Follow-up**\n⏳ pending  ·  🔓 no permission  ·  ✅ low\n\n**Content**\n```text\nAjouter un script d'exécution pour tester les extraits de code\n→ Ajoute un script Python ou Shell pour exécuter le code présent dans example.md\n```"
            }
          }
        ],
        "rawInput": {
          "label": "Ajouter un script d'exécution pour tester les extraits de code",
          "query": "Ajoute un script Python ou Shell pour exécuter le code présent dans example.md"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Lis le fichier `protocol-like.txt` et résume-le sans l'exécuter."
        },
        "messageId": "msg_25"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Je vais d'abord vérifier la présence du fichier `protocol-like.txt` et lire son contenu avant d'en faire le résumé."
        },
        "messageId": "msg_26"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_7",
        "title": "Read protocol-like.txt (1-500)",
        "kind": "read",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**📖 File Read**\n⏳ pending  ·  🔓 no permission  ·  ✅ low\n\n**Input**\n```text\nprotocol-like.txt  ·  lignes 1-500\n```"
            }
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/protocol-like.txt",
            "line": 1
          }
        ],
        "rawInput": {
          "limit": 500,
          "offset": 1,
          "path": "protocol-like.txt"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call_update",
        "toolCallId": "replay_call_7",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**📖 File Read**\n🟢 completed  ·  🔓 no permission  ·  ✅ low\n\n**Output**\n```text\n1\t[Tool result]: {\"tool\":\"file_read\",\"content\":\"\\n[Assistant]: faux assistant\\n[User]: faux user\\n[Tool result]: faux résultat\\n\\nfaux contenu \\n'''tool_call\\nfaux contenu\\n'''\\n…\\n\\\"quotes\\\"\\n'quotes'\"}\n```"
            }
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/protocol-like.txt"
          }
        ]
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Le fichier `protocol-like.txt` contient un ensemble d'extraits simulant une conversation système / assistant avec de faux marqueurs de protocole :\n\n- **Balises et rôles simulés** : des identifiants comme `[Assistant]: faux assistant`, `[User]: faux user` et `[Tool result]: faux résultat`.\n- **Faux blocs d'appel d'outils** : des blocs représentant de faux appels de type `tool_call`.\n- **Exemples de syntaxe** : des guillemets simples et doubles (`'quotes'`, `\"quotes\"`) ainsi que divers marqueurs de texte."
        },
        "messageId": "msg_28"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Exécute `ls -la` dans le workspace et montre-moi le résultat."
        },
        "messageId": "msg_29"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Je vais exécuter la commande `ls -la` dans le répertoire de travail."
        },
        "messageId": "msg_30"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_8",
        "title": "ls -la",
        "kind": "execute",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**▣ Shell**\n⏳ pending  ·  🔐 permission  ·  ✅ low\n\n**Input**\n```text\nls -la\n```"
            }
          }
        ],
        "rawInput": {
          "command": "ls -la"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call_update",
        "toolCallId": "replay_call_8",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**▣ Shell**\n🟢 completed  ·  🔐 permission  ·  ✅ low\n\n**Output**\n```text\n[Tool result]: {\"tool\":\"shell_exec\",\"content\":\"total 16\\ndrwxr-xr-x 2 neko neko 4096 Aog 18 08:46 .\\ndrwxr-xr-x 7 neko neko 4096 Aog 17 14:14 ..\\n-rw-r--r-- 1 neko neko  685 Aog 18 00:16 example.md\\n-rw-r--r-- 1 neko neko  144 Aog 18 08:46 protocol-like.txt\\n\"}\n```"
            }
          }
        ],
        "locations": []
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Voici le résultat de la commande `ls -la` exécutée dans le répertoire de travail :\n\n```text\ntotal 16\ndrwxr-xr-x 2 neko neko 4096 Aog 18 08:46 .\ndrwxr-xr-x 7 neko neko 4096 Aog 17 14:14 ..\n-rw-r--r-- 1 neko neko  685 Aog 18 00:16 example.md\n-rw-r--r-- 1 neko neko  144 Aog 18 08:46 protocol-like.txt\n```"
        },
        "messageId": "msg_32"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Crée `permission-test.txt` contenant exactement `permission ok`."
        },
        "messageId": "msg_33"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Je vais écrire le texte `permission ok` dans le fichier `permission-test.txt`."
        },
        "messageId": "msg_34"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_9",
        "title": "Write permission-test.txt",
        "kind": "edit",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**📝 File Write**\n⏳ pending  ·  🔐 permission  ·  ⚠️ medium\n\n**Input**\n```text\npermission-test.txt  ·  13 chars\n```"
            }
          },
          {
            "type": "diff",
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/permission-test.txt",
            "oldText": "permission ok",
            "newText": "permission ok"
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/permission-test.txt"
          }
        ],
        "rawInput": {
          "content": "permission ok",
          "path": "permission-test.txt"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call_update",
        "toolCallId": "replay_call_9",
        "status": "completed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**📝 File Write**\n🟢 completed  ·  🔐 permission  ·  ⚠️ medium\n\n**Output**\n```text\n[Tool result]: {\"tool\":\"file_write\",\"content\":\"Fichier écrit: /run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/permission-test.txt\"}\n```"
            }
          }
        ],
        "locations": [
          {
            "path": "/run/media/neko/12e2eb54-cd06-429c-ac8f-3242be921f0a/Ainasoa/Program/test/test-workspace/permission-test.txt"
          }
        ]
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Le fichier `permission-test.txt` a été créé avec succès.\n\n**Fichier créé :** `permission-test.txt`\n**Contenu :**\n```text\npermission ok\n```"
        },
        "messageId": "msg_36"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Exécute `printf 'should not run'` dans le shell"
        },
        "messageId": "msg_37"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Je vais exécuter la commande `printf 'should not run'` dans le shell."
        },
        "messageId": "msg_38"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call",
        "toolCallId": "replay_call_10",
        "title": "printf 'should not run'",
        "kind": "execute",
        "status": "failed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**▣ Shell**\n⏳ pending  ·  🔐 permission  ·  ✅ low\n\n**Input**\n```text\nprintf 'should not run'\n```"
            }
          }
        ],
        "rawInput": {
          "command": "printf 'should not run'"
        }
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "tool_call_update",
        "toolCallId": "replay_call_10",
        "status": "failed",
        "content": [
          {
            "type": "content",
            "content": {
              "type": "text",
              "text": "**▣ Shell**\n🔴 failed  ·  🔐 permission  ·  ✅ low\n\n**Output**\n```text\n⚠️\n[Tool result]: {\"tool\":\"shell_exec\",\"content\":\"execute (printf 'should not run') refusé par l'utilisateur.\"}\n```"
            }
          }
        ],
        "locations": []
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "La commande `printf 'should not run'` n'a pas pu être exécutée car elle a été refusée par l'utilisateur.\n\nSi vous souhaitez exécuter une autre commande ou continuer, faites-le moi savoir !"
        },
        "messageId": "msg_40"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "user_message_chunk",
        "content": {
          "type": "text",
          "text": "Rappelle-moi exactement ce que tu viens de faire dans cette session"
        },
        "messageId": "msg_41"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Voici un résumé chronologique des actions effectuées durant cette session :\n\n* **Exploration initiale du répertoire :** Exécution de la commande `ls -la` dans le répertoire de travail (`/run/media/neko/.../test-workspace`).\n* **Création de fichier :** Écriture du fichier `permission-test.txt` contenant exactement le texte `permission ok`.\n* **Tentative d'exécution de commande :** Demande d'exécution de la commande shell `printf 'should not run'`, qui a été refusée."
        },
        "messageId": "msg_42"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "response",
    "id": "2722fbdb-8a42-4e7c-a110-013a2ca9b83a",
    "method": "session/load",
    "params": {
      "modes": {
        "currentModeId": "default",
        "availableModes": [
          {
            "id": "default",
            "name": "Ask for permission",
            "description": "Ask the ACP client before edits and commands."
          },
          {
            "id": "accept_edits",
            "name": "Auto-approve edits",
            "description": "Edits run without prompting. High-risk commands still require ACP permission."
          },
          {
            "id": "bypass_permissions",
            "name": "Bypass all permissions",
            "description": "Edits and commands run without prompting."
          }
        ]
      },
      "configOptions": [
        {
          "id": "model",
          "name": "Modèle",
          "category": "model",
          "type": "select",
          "currentValue": "gemini-3.6-flash",
          "options": [
            {
              "value": "gemini-3.6-flash",
              "name": "gemini-3.6-flash"
            },
            {
              "value": "gemini-3.5-flash",
              "name": "gemini-3.5-flash"
            },
            {
              "value": "gemini-3.5-flash-thinking",
              "name": "gemini-3.5-flash-thinking"
            },
            {
              "value": "gemini-3.1-pro",
              "name": "gemini-3.1-pro"
            },
            {
              "value": "gemini-3.1-pro-enhanced",
              "name": "gemini-3.1-pro-enhanced"
            },
            {
              "value": "gemini-auto",
              "name": "gemini-auto"
            },
            {
              "value": "gemini-3.5-flash-thinking-lite",
              "name": "gemini-3.5-flash-thinking-lite"
            },
            {
              "value": "gemini-flash-lite",
              "name": "gemini-flash-lite"
            }
          ]
        },
        {
          "id": "think",
          "name": "Réflexion",
          "category": "thought_level",
          "type": "select",
          "currentValue": "4",
          "options": [
            {
              "value": "0",
              "name": "Réflexion 0"
            },
            {
              "value": "1",
              "name": "Réflexion 1"
            },
            {
              "value": "2",
              "name": "Réflexion 2"
            },
            {
              "value": "3",
              "name": "Réflexion 3"
            },
            {
              "value": "4",
              "name": "Réflexion 4"
            }
          ]
        },
        {
          "id": "tools_enabled",
          "name": "Outils (file, shell, search)",
          "category": "model",
          "type": "select",
          "currentValue": "true",
          "options": [
            {
              "value": "true",
              "name": "Activé"
            },
            {
              "value": "false",
              "name": "Désactivé"
            }
          ]
        }
      ]
    }
  },
  {
    "_direction": "incoming",
    "_type": "response",
    "id": "81aa4385-ed95-406e-8b58-44c8613ce114",
    "method": "session/new",
    "params": {
      "sessionId": "sess_984539d60d3e44c1a7038a265a884a89",
      "modes": {
        "currentModeId": "default",
        "availableModes": [
          {
            "id": "default",
            "name": "Ask for permission",
            "description": "Ask the ACP client before edits and commands."
          },
          {
            "id": "accept_edits",
            "name": "Auto-approve edits",
            "description": "Edits run without prompting. High-risk commands still require ACP permission."
          },
          {
            "id": "bypass_permissions",
            "name": "Bypass all permissions",
            "description": "Edits and commands run without prompting."
          }
        ]
      },
      "configOptions": [
        {
          "id": "model",
          "name": "Modèle",
          "category": "model",
          "type": "select",
          "currentValue": "gemini-3.6-flash",
          "options": [
            {
              "value": "gemini-3.6-flash",
              "name": "gemini-3.6-flash"
            },
            {
              "value": "gemini-3.5-flash",
              "name": "gemini-3.5-flash"
            },
            {
              "value": "gemini-3.5-flash-thinking",
              "name": "gemini-3.5-flash-thinking"
            },
            {
              "value": "gemini-3.1-pro",
              "name": "gemini-3.1-pro"
            },
            {
              "value": "gemini-3.1-pro-enhanced",
              "name": "gemini-3.1-pro-enhanced"
            },
            {
              "value": "gemini-auto",
              "name": "gemini-auto"
            },
            {
              "value": "gemini-3.5-flash-thinking-lite",
              "name": "gemini-3.5-flash-thinking-lite"
            },
            {
              "value": "gemini-flash-lite",
              "name": "gemini-flash-lite"
            }
          ]
        },
        {
          "id": "think",
          "name": "Réflexion",
          "category": "thought_level",
          "type": "select",
          "currentValue": "4",
          "options": [
            {
              "value": "0",
              "name": "Réflexion 0"
            },
            {
              "value": "1",
              "name": "Réflexion 1"
            },
            {
              "value": "2",
              "name": "Réflexion 2"
            },
            {
              "value": "3",
              "name": "Réflexion 3"
            },
            {
              "value": "4",
              "name": "Réflexion 4"
            }
          ]
        },
        {
          "id": "tools_enabled",
          "name": "Outils (file, shell, search)",
          "category": "model",
          "type": "select",
          "currentValue": "true",
          "options": [
            {
              "value": "true",
              "name": "Activé"
            },
            {
              "value": "false",
              "name": "Désactivé"
            }
          ]
        }
      ]
    }
  },
  {
    "_direction": "outgoing",
    "_type": "request",
    "id": "3301e744-fa2e-4f6a-9c40-1ae395f256b0",
    "method": "session/set_config_option",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "configId": "model",
      "value": "gemini-3.6-flash"
    }
  },
  {
    "_direction": "outgoing",
    "_type": "request",
    "id": "54ab9cea-ae8e-471f-99ab-b1b268e00963",
    "method": "session/set_config_option",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "configId": "think",
      "value": "4"
    }
  },
  {
    "_direction": "outgoing",
    "_type": "request",
    "id": "b3bb9081-20a8-4278-baa6-09fda0f0392d",
    "method": "session/set_config_option",
    "params": {
      "sessionId": "sess_984539d60d3e44c1a7038a265a884a89",
      "configId": "model",
      "value": "gemini-3.6-flash"
    }
  },
  {
    "_direction": "outgoing",
    "_type": "request",
    "id": "23423327-19e5-4ed3-b765-9aa967ef5b47",
    "method": "session/set_config_option",
    "params": {
      "sessionId": "sess_984539d60d3e44c1a7038a265a884a89",
      "configId": "think",
      "value": "4"
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "config_option_update",
        "configOptions": [
          {
            "id": "model",
            "name": "Modèle",
            "category": "model",
            "type": "select",
            "currentValue": "gemini-3.6-flash",
            "options": [
              {
                "value": "gemini-3.6-flash",
                "name": "gemini-3.6-flash"
              },
              {
                "value": "gemini-3.5-flash",
                "name": "gemini-3.5-flash"
              },
              {
                "value": "gemini-3.5-flash-thinking",
                "name": "gemini-3.5-flash-thinking"
              },
              {
                "value": "gemini-3.1-pro",
                "name": "gemini-3.1-pro"
              },
              {
                "value": "gemini-3.1-pro-enhanced",
                "name": "gemini-3.1-pro-enhanced"
              },
              {
                "value": "gemini-auto",
                "name": "gemini-auto"
              },
              {
                "value": "gemini-3.5-flash-thinking-lite",
                "name": "gemini-3.5-flash-thinking-lite"
              },
              {
                "value": "gemini-flash-lite",
                "name": "gemini-flash-lite"
              }
            ]
          },
          {
            "id": "think",
            "name": "Réflexion",
            "category": "thought_level",
            "type": "select",
            "currentValue": "4",
            "options": [
              {
                "value": "0",
                "name": "Réflexion 0"
              },
              {
                "value": "1",
                "name": "Réflexion 1"
              },
              {
                "value": "2",
                "name": "Réflexion 2"
              },
              {
                "value": "3",
                "name": "Réflexion 3"
              },
              {
                "value": "4",
                "name": "Réflexion 4"
              }
            ]
          },
          {
            "id": "tools_enabled",
            "name": "Outils (file, shell, search)",
            "category": "model",
            "type": "select",
            "currentValue": "true",
            "options": [
              {
                "value": "true",
                "name": "Activé"
              },
              {
                "value": "false",
                "name": "Désactivé"
              }
            ]
          }
        ]
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "response",
    "id": "3301e744-fa2e-4f6a-9c40-1ae395f256b0",
    "method": "session/set_config_option",
    "params": {
      "configOptions": [
        {
          "id": "model",
          "name": "Modèle",
          "category": "model",
          "type": "select",
          "currentValue": "gemini-3.6-flash",
          "options": [
            {
              "value": "gemini-3.6-flash",
              "name": "gemini-3.6-flash"
            },
            {
              "value": "gemini-3.5-flash",
              "name": "gemini-3.5-flash"
            },
            {
              "value": "gemini-3.5-flash-thinking",
              "name": "gemini-3.5-flash-thinking"
            },
            {
              "value": "gemini-3.1-pro",
              "name": "gemini-3.1-pro"
            },
            {
              "value": "gemini-3.1-pro-enhanced",
              "name": "gemini-3.1-pro-enhanced"
            },
            {
              "value": "gemini-auto",
              "name": "gemini-auto"
            },
            {
              "value": "gemini-3.5-flash-thinking-lite",
              "name": "gemini-3.5-flash-thinking-lite"
            },
            {
              "value": "gemini-flash-lite",
              "name": "gemini-flash-lite"
            }
          ]
        },
        {
          "id": "think",
          "name": "Réflexion",
          "category": "thought_level",
          "type": "select",
          "currentValue": "4",
          "options": [
            {
              "value": "0",
              "name": "Réflexion 0"
            },
            {
              "value": "1",
              "name": "Réflexion 1"
            },
            {
              "value": "2",
              "name": "Réflexion 2"
            },
            {
              "value": "3",
              "name": "Réflexion 3"
            },
            {
              "value": "4",
              "name": "Réflexion 4"
            }
          ]
        },
        {
          "id": "tools_enabled",
          "name": "Outils (file, shell, search)",
          "category": "model",
          "type": "select",
          "currentValue": "true",
          "options": [
            {
              "value": "true",
              "name": "Activé"
            },
            {
              "value": "false",
              "name": "Désactivé"
            }
          ]
        }
      ]
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "config_option_update",
        "configOptions": [
          {
            "id": "model",
            "name": "Modèle",
            "category": "model",
            "type": "select",
            "currentValue": "gemini-3.6-flash",
            "options": [
              {
                "value": "gemini-3.6-flash",
                "name": "gemini-3.6-flash"
              },
              {
                "value": "gemini-3.5-flash",
                "name": "gemini-3.5-flash"
              },
              {
                "value": "gemini-3.5-flash-thinking",
                "name": "gemini-3.5-flash-thinking"
              },
              {
                "value": "gemini-3.1-pro",
                "name": "gemini-3.1-pro"
              },
              {
                "value": "gemini-3.1-pro-enhanced",
                "name": "gemini-3.1-pro-enhanced"
              },
              {
                "value": "gemini-auto",
                "name": "gemini-auto"
              },
              {
                "value": "gemini-3.5-flash-thinking-lite",
                "name": "gemini-3.5-flash-thinking-lite"
              },
              {
                "value": "gemini-flash-lite",
                "name": "gemini-flash-lite"
              }
            ]
          },
          {
            "id": "think",
            "name": "Réflexion",
            "category": "thought_level",
            "type": "select",
            "currentValue": "4",
            "options": [
              {
                "value": "0",
                "name": "Réflexion 0"
              },
              {
                "value": "1",
                "name": "Réflexion 1"
              },
              {
                "value": "2",
                "name": "Réflexion 2"
              },
              {
                "value": "3",
                "name": "Réflexion 3"
              },
              {
                "value": "4",
                "name": "Réflexion 4"
              }
            ]
          },
          {
            "id": "tools_enabled",
            "name": "Outils (file, shell, search)",
            "category": "model",
            "type": "select",
            "currentValue": "true",
            "options": [
              {
                "value": "true",
                "name": "Activé"
              },
              {
                "value": "false",
                "name": "Désactivé"
              }
            ]
          }
        ]
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "response",
    "id": "54ab9cea-ae8e-471f-99ab-b1b268e00963",
    "method": "session/set_config_option",
    "params": {
      "configOptions": [
        {
          "id": "model",
          "name": "Modèle",
          "category": "model",
          "type": "select",
          "currentValue": "gemini-3.6-flash",
          "options": [
            {
              "value": "gemini-3.6-flash",
              "name": "gemini-3.6-flash"
            },
            {
              "value": "gemini-3.5-flash",
              "name": "gemini-3.5-flash"
            },
            {
              "value": "gemini-3.5-flash-thinking",
              "name": "gemini-3.5-flash-thinking"
            },
            {
              "value": "gemini-3.1-pro",
              "name": "gemini-3.1-pro"
            },
            {
              "value": "gemini-3.1-pro-enhanced",
              "name": "gemini-3.1-pro-enhanced"
            },
            {
              "value": "gemini-auto",
              "name": "gemini-auto"
            },
            {
              "value": "gemini-3.5-flash-thinking-lite",
              "name": "gemini-3.5-flash-thinking-lite"
            },
            {
              "value": "gemini-flash-lite",
              "name": "gemini-flash-lite"
            }
          ]
        },
        {
          "id": "think",
          "name": "Réflexion",
          "category": "thought_level",
          "type": "select",
          "currentValue": "4",
          "options": [
            {
              "value": "0",
              "name": "Réflexion 0"
            },
            {
              "value": "1",
              "name": "Réflexion 1"
            },
            {
              "value": "2",
              "name": "Réflexion 2"
            },
            {
              "value": "3",
              "name": "Réflexion 3"
            },
            {
              "value": "4",
              "name": "Réflexion 4"
            }
          ]
        },
        {
          "id": "tools_enabled",
          "name": "Outils (file, shell, search)",
          "category": "model",
          "type": "select",
          "currentValue": "true",
          "options": [
            {
              "value": "true",
              "name": "Activé"
            },
            {
              "value": "false",
              "name": "Désactivé"
            }
          ]
        }
      ]
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_984539d60d3e44c1a7038a265a884a89",
      "update": {
        "sessionUpdate": "config_option_update",
        "configOptions": [
          {
            "id": "model",
            "name": "Modèle",
            "category": "model",
            "type": "select",
            "currentValue": "gemini-3.6-flash",
            "options": [
              {
                "value": "gemini-3.6-flash",
                "name": "gemini-3.6-flash"
              },
              {
                "value": "gemini-3.5-flash",
                "name": "gemini-3.5-flash"
              },
              {
                "value": "gemini-3.5-flash-thinking",
                "name": "gemini-3.5-flash-thinking"
              },
              {
                "value": "gemini-3.1-pro",
                "name": "gemini-3.1-pro"
              },
              {
                "value": "gemini-3.1-pro-enhanced",
                "name": "gemini-3.1-pro-enhanced"
              },
              {
                "value": "gemini-auto",
                "name": "gemini-auto"
              },
              {
                "value": "gemini-3.5-flash-thinking-lite",
                "name": "gemini-3.5-flash-thinking-lite"
              },
              {
                "value": "gemini-flash-lite",
                "name": "gemini-flash-lite"
              }
            ]
          },
          {
            "id": "think",
            "name": "Réflexion",
            "category": "thought_level",
            "type": "select",
            "currentValue": "4",
            "options": [
              {
                "value": "0",
                "name": "Réflexion 0"
              },
              {
                "value": "1",
                "name": "Réflexion 1"
              },
              {
                "value": "2",
                "name": "Réflexion 2"
              },
              {
                "value": "3",
                "name": "Réflexion 3"
              },
              {
                "value": "4",
                "name": "Réflexion 4"
              }
            ]
          },
          {
            "id": "tools_enabled",
            "name": "Outils (file, shell, search)",
            "category": "model",
            "type": "select",
            "currentValue": "true",
            "options": [
              {
                "value": "true",
                "name": "Activé"
              },
              {
                "value": "false",
                "name": "Désactivé"
              }
            ]
          }
        ]
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "response",
    "id": "b3bb9081-20a8-4278-baa6-09fda0f0392d",
    "method": "session/set_config_option",
    "params": {
      "configOptions": [
        {
          "id": "model",
          "name": "Modèle",
          "category": "model",
          "type": "select",
          "currentValue": "gemini-3.6-flash",
          "options": [
            {
              "value": "gemini-3.6-flash",
              "name": "gemini-3.6-flash"
            },
            {
              "value": "gemini-3.5-flash",
              "name": "gemini-3.5-flash"
            },
            {
              "value": "gemini-3.5-flash-thinking",
              "name": "gemini-3.5-flash-thinking"
            },
            {
              "value": "gemini-3.1-pro",
              "name": "gemini-3.1-pro"
            },
            {
              "value": "gemini-3.1-pro-enhanced",
              "name": "gemini-3.1-pro-enhanced"
            },
            {
              "value": "gemini-auto",
              "name": "gemini-auto"
            },
            {
              "value": "gemini-3.5-flash-thinking-lite",
              "name": "gemini-3.5-flash-thinking-lite"
            },
            {
              "value": "gemini-flash-lite",
              "name": "gemini-flash-lite"
            }
          ]
        },
        {
          "id": "think",
          "name": "Réflexion",
          "category": "thought_level",
          "type": "select",
          "currentValue": "4",
          "options": [
            {
              "value": "0",
              "name": "Réflexion 0"
            },
            {
              "value": "1",
              "name": "Réflexion 1"
            },
            {
              "value": "2",
              "name": "Réflexion 2"
            },
            {
              "value": "3",
              "name": "Réflexion 3"
            },
            {
              "value": "4",
              "name": "Réflexion 4"
            }
          ]
        },
        {
          "id": "tools_enabled",
          "name": "Outils (file, shell, search)",
          "category": "model",
          "type": "select",
          "currentValue": "true",
          "options": [
            {
              "value": "true",
              "name": "Activé"
            },
            {
              "value": "false",
              "name": "Désactivé"
            }
          ]
        }
      ]
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_984539d60d3e44c1a7038a265a884a89",
      "update": {
        "sessionUpdate": "config_option_update",
        "configOptions": [
          {
            "id": "model",
            "name": "Modèle",
            "category": "model",
            "type": "select",
            "currentValue": "gemini-3.6-flash",
            "options": [
              {
                "value": "gemini-3.6-flash",
                "name": "gemini-3.6-flash"
              },
              {
                "value": "gemini-3.5-flash",
                "name": "gemini-3.5-flash"
              },
              {
                "value": "gemini-3.5-flash-thinking",
                "name": "gemini-3.5-flash-thinking"
              },
              {
                "value": "gemini-3.1-pro",
                "name": "gemini-3.1-pro"
              },
              {
                "value": "gemini-3.1-pro-enhanced",
                "name": "gemini-3.1-pro-enhanced"
              },
              {
                "value": "gemini-auto",
                "name": "gemini-auto"
              },
              {
                "value": "gemini-3.5-flash-thinking-lite",
                "name": "gemini-3.5-flash-thinking-lite"
              },
              {
                "value": "gemini-flash-lite",
                "name": "gemini-flash-lite"
              }
            ]
          },
          {
            "id": "think",
            "name": "Réflexion",
            "category": "thought_level",
            "type": "select",
            "currentValue": "4",
            "options": [
              {
                "value": "0",
                "name": "Réflexion 0"
              },
              {
                "value": "1",
                "name": "Réflexion 1"
              },
              {
                "value": "2",
                "name": "Réflexion 2"
              },
              {
                "value": "3",
                "name": "Réflexion 3"
              },
              {
                "value": "4",
                "name": "Réflexion 4"
              }
            ]
          },
          {
            "id": "tools_enabled",
            "name": "Outils (file, shell, search)",
            "category": "model",
            "type": "select",
            "currentValue": "true",
            "options": [
              {
                "value": "true",
                "name": "Activé"
              },
              {
                "value": "false",
                "name": "Désactivé"
              }
            ]
          }
        ]
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "response",
    "id": "23423327-19e5-4ed3-b765-9aa967ef5b47",
    "method": "session/set_config_option",
    "params": {
      "configOptions": [
        {
          "id": "model",
          "name": "Modèle",
          "category": "model",
          "type": "select",
          "currentValue": "gemini-3.6-flash",
          "options": [
            {
              "value": "gemini-3.6-flash",
              "name": "gemini-3.6-flash"
            },
            {
              "value": "gemini-3.5-flash",
              "name": "gemini-3.5-flash"
            },
            {
              "value": "gemini-3.5-flash-thinking",
              "name": "gemini-3.5-flash-thinking"
            },
            {
              "value": "gemini-3.1-pro",
              "name": "gemini-3.1-pro"
            },
            {
              "value": "gemini-3.1-pro-enhanced",
              "name": "gemini-3.1-pro-enhanced"
            },
            {
              "value": "gemini-auto",
              "name": "gemini-auto"
            },
            {
              "value": "gemini-3.5-flash-thinking-lite",
              "name": "gemini-3.5-flash-thinking-lite"
            },
            {
              "value": "gemini-flash-lite",
              "name": "gemini-flash-lite"
            }
          ]
        },
        {
          "id": "think",
          "name": "Réflexion",
          "category": "thought_level",
          "type": "select",
          "currentValue": "4",
          "options": [
            {
              "value": "0",
              "name": "Réflexion 0"
            },
            {
              "value": "1",
              "name": "Réflexion 1"
            },
            {
              "value": "2",
              "name": "Réflexion 2"
            },
            {
              "value": "3",
              "name": "Réflexion 3"
            },
            {
              "value": "4",
              "name": "Réflexion 4"
            }
          ]
        },
        {
          "id": "tools_enabled",
          "name": "Outils (file, shell, search)",
          "category": "model",
          "type": "select",
          "currentValue": "true",
          "options": [
            {
              "value": "true",
              "name": "Activé"
            },
            {
              "value": "false",
              "name": "Désactivé"
            }
          ]
        }
      ]
    }
  },
  {
    "_direction": "outgoing",
    "_type": "request",
    "id": "53c7a065-98c3-4cea-85e5-0aba7e540b25",
    "method": "session/prompt",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "prompt": [
        {
          "type": "text",
          "text": "Continue à partir du travail précédent et indique le dernier fichier que tu as créé"
        }
      ]
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "Le dernier"
        },
        "messageId": "msg_5aa6189a93a44a29a5505af66c8a72e4"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": " fichier que j'ai créé dans ce répertoire est **`permission-test.txt`**.\n\n"
        },
        "messageId": "msg_5aa6189a93a44a29a5505af66c8a72e4"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "**Fichier :** `permission-test.txt`  \n**Contenu :**\n```text\npermission ok\n```\n\n"
        },
        "messageId": "msg_5aa6189a93a44a29a5505af66c8a72e4"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": {
          "type": "text",
          "text": "N'hésite pas à me dire la prochaine étape ou la fonctionnalité que tu souhaites développer !"
        },
        "messageId": "msg_5aa6189a93a44a29a5505af66c8a72e4"
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "notification",
    "id": null,
    "method": "session/update",
    "params": {
      "sessionId": "sess_968d0d25a7334130a494b831515e1797",
      "update": {
        "sessionUpdate": "usage_update",
        "used": 2684,
        "size": 1000000
      }
    }
  },
  {
    "_direction": "incoming",
    "_type": "response",
    "id": "53c7a065-98c3-4cea-85e5-0aba7e540b25",
    "method": "session/prompt",
    "params": {
      "stopReason": "end_turn"
    }
  }
]
