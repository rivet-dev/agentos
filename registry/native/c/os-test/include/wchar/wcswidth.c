/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <wchar.h>
#ifdef wcswidth
#undef wcswidth
#endif
int (*foo)(const wchar_t *, size_t) = wcswidth;
int main(void) { return 0; }
