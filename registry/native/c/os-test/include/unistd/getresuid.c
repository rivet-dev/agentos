/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <unistd.h>
#ifdef getresuid
#undef getresuid
#endif
int (*foo)(uid_t *restrict, uid_t *restrict, uid_t *restrict) = getresuid;
int main(void) { return 0; }
