#!/usr/bin/env python3
"""
Agent Runner - Executes tasks using the Anthropic API with tool use.

Uses the anthropic Python SDK directly for structured tool-use patterns,
replacing the non-existent 'claude_agent_sdk' package.

Requires: pip install anthropic
"""

import sys
import json
import asyncio
import os

# Available tools the agent can use
TOOLS = [
    {
        "name": "web_search",
        "description": "Search the web for information relevant to the task.",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                }
            },
            "required": ["query"]
        }
    },
    {
        "name": "read_file",
        "description": "Read the contents of a file on the local filesystem.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to read"
                }
            },
            "required": ["path"]
        }
    },
    {
        "name": "write_file",
        "description": "Write content to a file on the local filesystem.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to write to"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                }
            },
            "required": ["path", "content"]
        }
    },
    {
        "name": "run_command",
        "description": "Run a shell command and return its output. Use with caution.",
        "input_schema": {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to run"
                }
            },
            "required": ["command"]
        }
    },
    {
        "name": "report_critical_point",
        "description": "Report a critical decision point that needs user approval (purchases, form submissions, emails, etc).",
        "input_schema": {
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "What action needs approval"
                },
                "risk_level": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "description": "How risky is this action"
                }
            },
            "required": ["action", "risk_level"]
        }
    }
]


def execute_tool(name: str, tool_input: dict) -> str:
    """Execute a tool and return the result as a string."""
    import subprocess

    if name == "web_search":
        return json.dumps({"note": "Web search not available in local mode", "query": tool_input.get("query", "")})

    elif name == "read_file":
        try:
            with open(tool_input["path"], "r") as f:
                content = f.read()
            return content[:5000]  # Truncate long files
        except Exception as e:
            return f"Error reading file: {e}"

    elif name == "write_file":
        try:
            with open(tool_input["path"], "w") as f:
                f.write(tool_input["content"])
            return f"Successfully wrote to {tool_input['path']}"
        except Exception as e:
            return f"Error writing file: {e}"

    elif name == "run_command":
        try:
            result = subprocess.run(
                tool_input["command"],
                shell=True,
                capture_output=True,
                text=True,
                timeout=30
            )
            output = result.stdout[:3000] if result.stdout else ""
            if result.stderr:
                output += f"\nSTDERR: {result.stderr[:1000]}"
            return output or "(no output)"
        except subprocess.TimeoutExpired:
            return "Command timed out after 30 seconds"
        except Exception as e:
            return f"Error running command: {e}"

    elif name == "report_critical_point":
        return json.dumps({
            "status": "critical_point",
            "action": tool_input.get("action", ""),
            "risk": tool_input.get("risk_level", "unknown"),
            "message": "Awaiting user approval"
        })

    return f"Unknown tool: {name}"


async def run_task(task_description: str):
    """Execute a task using the Anthropic Messages API with tool use."""
    try:
        import anthropic
    except ImportError:
        print(json.dumps({
            "status": "error",
            "message": "anthropic package not installed. Run: pip install anthropic"
        }))
        return

    api_key = os.environ.get("ANTHROPIC_API_KEY", "")
    if not api_key:
        print(json.dumps({
            "status": "error",
            "message": "ANTHROPIC_API_KEY not set. Get one from https://console.anthropic.com/"
        }))
        return

    client = anthropic.Anthropic(api_key=api_key)

    print(json.dumps({
        "status": "started",
        "message": f"Executing: {task_description}"
    }), flush=True)

    messages = [
        {
            "role": "user",
            "content": f"""You are helping a user complete a task on their macOS computer.

Task: {task_description}

Instructions:
- Complete this task step by step
- Use the available tools to accomplish the goal
- Stop and use report_critical_point for any risky actions (purchases, sending emails, deleting data)
- Be efficient and focused on the goal
- Provide clear status updates as you work"""
        }
    ]

    max_iterations = 10
    for iteration in range(max_iterations):
        try:
            response = client.messages.create(
                model="claude-sonnet-4-6",
                max_tokens=4096,
                tools=TOOLS,
                messages=messages,
            )
        except Exception as e:
            print(json.dumps({
                "status": "error",
                "message": f"API call failed: {e}"
            }), flush=True)
            return

        # Process the response
        assistant_content = response.content
        has_tool_use = False

        for block in assistant_content:
            if block.type == "text":
                print(json.dumps({
                    "status": "thinking",
                    "text": block.text[:500]
                }), flush=True)

            elif block.type == "tool_use":
                has_tool_use = True
                tool_name = block.name
                tool_input = block.input

                print(json.dumps({
                    "status": "tool_use",
                    "tool": tool_name,
                    "input": str(tool_input)[:200]
                }), flush=True)

                # Execute the tool
                tool_result = execute_tool(tool_name, tool_input)

                print(json.dumps({
                    "status": "tool_result",
                    "tool": tool_name,
                    "result": tool_result[:300]
                }), flush=True)

                # Check for critical point
                if tool_name == "report_critical_point":
                    print(json.dumps({
                        "status": "critical_point",
                        "message": f"Critical point: {tool_input.get('action', 'Unknown action')}",
                        "risk": tool_input.get("risk_level", "unknown")
                    }), flush=True)
                    return

        # Build the next messages
        messages.append({"role": "assistant", "content": assistant_content})

        if has_tool_use:
            # Collect all tool results
            tool_results = []
            for block in assistant_content:
                if block.type == "tool_use":
                    result = execute_tool(block.name, block.input)
                    tool_results.append({
                        "type": "tool_result",
                        "tool_use_id": block.id,
                        "content": result
                    })
            messages.append({"role": "user", "content": tool_results})
        else:
            # No tool use = agent is done
            break

        if response.stop_reason == "end_turn":
            break

    print(json.dumps({
        "status": "completed",
        "message": "Task completed successfully"
    }), flush=True)


def show_setup_instructions():
    """Show setup instructions for the Anthropic agent."""
    instructions = """
╔══════════════════════════════════════════════════════════════════╗
║              ANTHROPIC AGENT SETUP INSTRUCTIONS                  ║
╠══════════════════════════════════════════════════════════════════╣
║                                                                  ║
║  1. Install the Anthropic Python SDK:                            ║
║     pip install anthropic                                        ║
║                                                                  ║
║  2. Set your Anthropic API key:                                  ║
║     export ANTHROPIC_API_KEY="your-key-here"                     ║
║                                                                  ║
║  3. Get your API key from:                                       ║
║     https://console.anthropic.com/                               ║
║                                                                  ║
║  For more info: https://docs.anthropic.com/                      ║
║                                                                  ║
╚══════════════════════════════════════════════════════════════════╝
"""
    print(instructions)
    print(json.dumps({
        "status": "setup_required",
        "message": "Anthropic SDK setup required. See instructions above."
    }))


if __name__ == "__main__":
    if len(sys.argv) < 2:
        show_setup_instructions()
        sys.exit(1)

    task = " ".join(sys.argv[1:])

    if task == "--setup":
        show_setup_instructions()
    else:
        asyncio.run(run_task(task))
