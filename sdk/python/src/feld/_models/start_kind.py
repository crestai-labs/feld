from enum import StrEnum


class StartKind(StrEnum):
    ATTACH = "attach"
    REPLACEMENT = "replacement"
    SCHEDULE = "schedule"
    START = "start"

    def __str__(self) -> str:
        return str(self.value)
