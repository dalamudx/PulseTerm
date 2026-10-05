# Fixed validation fixture, NOT the production integration script.
# Only used after sourcing a private synthetic user configuration.
if [[ ${PULSE_TEST_INJECTED:-0} == 1 ]]; then
    return 0
fi
PULSE_TEST_INJECTED=1
__pulse_probe_pre_prompt() {
    local status=$?
    if [[ ${_PULSE_SEEN:-0} == 1 ]]; then
        builtin printf '\033]133;D;%s\007' "$status"
    fi
    _PULSE_SEEN=1
    return "$status"
}
PROMPT_COMMAND=(__pulse_probe_pre_prompt "${PROMPT_COMMAND[@]}")
PS1='\[\e]133;A\a\]'"$PS1"'\[\e]133;B\a\]'
PS0=$'\e]133;C\a'"$PS0"
