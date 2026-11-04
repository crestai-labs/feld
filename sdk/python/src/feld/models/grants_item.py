from enum import StrEnum


class GrantsItem(StrEnum):
    CONTROL = "control"
    MANAGE_AGENT = "manage-agent"
    MANAGE_JOB = "manage-job"
    READ = "read"

    def __str__(self) -> str:
        return str(self.value)
