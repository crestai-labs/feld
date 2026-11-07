from enum import StrEnum


class Grant(StrEnum):
    CONTROL = "control"
    MANAGE_AGENT = "manage-agent"
    MANAGE_JOB = "manage-job"
    READ = "read"

    def __str__(self) -> str:
        return str(self.value)
