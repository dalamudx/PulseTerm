#ifndef PULSE_ECHO_NAVIGATION_H
#define PULSE_ECHO_NAVIGATION_H
#include <string>
// Ignore fragments only: renderer query, path and scheme must remain identical.
inline bool echo_same_document(const std::wstring &left, const std::wstring &right) {
    return left.substr(0, left.find(L'#')) == right.substr(0, right.find(L'#'));
}
class EchoNavigation {
    std::wstring target;
    unsigned long long id = 0;
    bool started = false;
public:
    void select(const std::wstring &uri) {target=uri;started=false;}
    bool begin(const std::wstring &uri, unsigned long long value) {
        if (!echo_same_document(target, uri)) return false;
        id=value;started=true;return true;
    }
    bool completes(unsigned long long value) const {return started && value==id;}
};
#endif
