from enum import StrEnum


class OutputPageLinesItemStream(StrEnum):
    STDERR = "stderr"
    STDOUT = "stdout"

    def __str__(self) -> str:
        return str(self.value)
