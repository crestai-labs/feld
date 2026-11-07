from enum import StrEnum


class ActorKind(StrEnum):
    AGENT = "agent"
    JOB = "job"

    def __str__(self) -> str:
        return str(self.value)
