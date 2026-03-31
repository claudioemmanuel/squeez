const SQUEEZ_BIN = `${process.env.HOME}/.claude/squeez/bin/squeez`;

export const SqueezPlugin = async () => {
  return {
    "tool.execute.before": async (input, output) => {
      if (input.tool === "bash") {
        const command = output.args?.command;
        if (!command || typeof command !== "string") return;
        if (command.startsWith(SQUEEZ_BIN)) return;
        if (command.includes("squeez wrap")) return;
        if (command.startsWith("--no-squeez")) return;
        output.args.command = `${SQUEEZ_BIN} wrap ${command}`;
      }
    },
  };
};
