from enum import StrEnum


class MetricStatus(StrEnum):
    NA = "n/a"
    REPORTED = "reported"
    UNAVAILABLE = "unavailable"

    def __str__(self) -> str:
        return str(self.value)
