/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <wchar.h>
#ifdef wcwidth
#undef wcwidth
#endif
int (*foo)(wchar_t) = wcwidth;
int main(void) { return 0; }
