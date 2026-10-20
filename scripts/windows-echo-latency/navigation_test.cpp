#include "navigation.h"
#include <cassert>
#include <cstdio>
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
    std::puts("navigation: unrelated/old completions rejected; exact document and fragment handling passed");
}
