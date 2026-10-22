#ifndef PULSE_ECHO_NAVIGATION_H
#define PULSE_ECHO_NAVIGATION_H
#include <string>
// NavigateToString may report about:blank or the exact encoded document.
inline constexpr wchar_t echo_memory_html[] = L"<!doctype html><meta charset='utf-8'><title>Startup control</title><p>WebView2 startup control</p>";
inline constexpr wchar_t echo_memory_uri[] = L"data:text/html;charset=utf-8;base64,PCFkb2N0eXBlIGh0bWw+PG1ldGEgY2hhcnNldD0ndXRmLTgnPjx0aXRsZT5TdGFydHVwIGNvbnRyb2w8L3RpdGxlPjxwPldlYlZpZXcyIHN0YXJ0dXAgY29udHJvbDwvcD4=";
// Ignore fragments only: renderer query, path and scheme must remain identical.
inline bool echo_same_document(const std::wstring &left, const std::wstring &right) {
    return left.substr(0, left.find(L'#')) == right.substr(0, right.find(L'#'));
}
class EchoNavigation {
    std::wstring target;
    unsigned long long id = 0;
    bool started = false;
    bool memory = false;
public:
    void select(const std::wstring &uri) {target=uri;started=false;memory=false;}
    void select_memory() {select(L"about:blank");memory=true;}
    bool begin(const std::wstring &uri, unsigned long long value) {
        if (!echo_same_document(target, uri) && !(memory && echo_same_document(echo_memory_uri, uri))) return false;
        id=value;started=true;return true;
    }
    bool completes(unsigned long long value) const {return started && value==id;}
};
#endif
