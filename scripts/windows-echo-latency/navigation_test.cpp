#include "navigation.h"
#include <cassert>
#include <cstdio>
static std::wstring base64_ascii(const std::wstring &text) {
    const wchar_t *alphabet = L"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::wstring encoded;
    unsigned value = 0, bits = 0;
    for (wchar_t ch : text) {
        assert(static_cast<unsigned>(ch) <= 127);
        value = (value << 8) | static_cast<unsigned>(ch); bits += 8;
        while (bits >= 6) {bits -= 6;encoded += alphabet[(value >> bits) & 63];}
    }
    if (bits) encoded += alphabet[(value << (6 - bits)) & 63];
    while (encoded.size() % 4) encoded += L'=';
    return encoded;
}
int main() {
    EchoNavigation nav;
    nav.select(L"file:///D:/kit%20name/echo.html?renderer=webgl#diagnostic=1");
    assert(!nav.completes(0));
    assert(!nav.begin(L"about:blank", 1));
    assert(!nav.completes(1));
    assert(nav.begin(L"file:///D:/kit%20name/echo.html?renderer=webgl", 2));
    assert(!nav.completes(1)); assert(nav.completes(2));
    assert(!nav.begin(L"about:blank",3)); assert(nav.completes(2));
    assert(!echo_same_document(L"file:///a?renderer=dom",L"file:///a?renderer=webgl"));
    assert(!echo_same_document(L"file:///a/echo.html",L"file:///b/echo.html"));
    assert(!echo_same_document(L"file:///a",L"https://a"));
    assert(echo_same_document(L"file:///a#diagnostic=1",L"file:///a#other"));
    nav.select(L"file:///new");assert(!nav.completes(2));
    const std::wstring memory_uri = echo_memory_uri;
    assert(memory_uri == L"data:text/html;charset=utf-8;base64," + base64_ascii(echo_memory_html));
    assert(!nav.begin(memory_uri, 3)); // File targets never accept an in-memory control.
    nav.select_memory(); assert(!nav.completes(2));
    assert(!nav.begin(L"file:///echo.html", 3));
    assert(!nav.begin(L"data:text/html;charset=utf-8;base64,b3RoZXI=", 3));
    assert(!nav.begin(L"data:text/plain;charset=utf-8;base64," + base64_ascii(echo_memory_html), 3));
    assert(nav.begin(memory_uri, 4)); assert(nav.completes(4)); assert(!nav.completes(3));
    assert(!nav.begin(memory_uri + L"?different", 5)); assert(nav.completes(4));
    assert(nav.begin(memory_uri + L"#fragment", 5)); assert(nav.completes(5)); assert(!nav.completes(4));
    assert(nav.begin(L"about:blank", 6)); assert(nav.completes(6));
    nav.select(L"file:///new"); assert(!nav.completes(6)); assert(!nav.begin(memory_uri, 7));
    std::puts("navigation: exact captured memory data URI/about:blank accepted only in memory mode; altered payload, file isolation and stale IDs passed");
}
