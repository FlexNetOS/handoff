#!/usr/bin/env bash
CHANGE="$1"

while grep "^- \[ \]" "openspec/changes/$CHANGE/tasks.md"; do
  claude --print --dangerously-skip-permissions "Read openspec/changes/$CHANGE/tasks.md, take the next unfinished task \
    in openspec/changes/$CHANGE/tasks.md, implement this task, \
    verify if the changes are correct (incl. Library-Constraints) \
    , and mark the task as completed."
done
