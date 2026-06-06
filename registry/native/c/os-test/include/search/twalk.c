/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <search.h>
#ifdef twalk
#undef twalk
#endif
void (*foo)(const posix_tnode *, void (*)(const posix_tnode *, VISIT, int)) = twalk;
int main(void) { return 0; }
