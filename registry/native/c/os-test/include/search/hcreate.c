/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <search.h>
#ifdef hcreate
#undef hcreate
#endif
int (*foo)(size_t) = hcreate;
int main(void) { return 0; }
