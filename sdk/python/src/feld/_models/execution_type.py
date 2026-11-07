from enum import StrEnum


class ExecutionType(StrEnum):
    AGENT_RUNTIME = "agent-runtime"
    PROGRAM = "program"

    def __str__(self) -> str:
        return str(self.value)
