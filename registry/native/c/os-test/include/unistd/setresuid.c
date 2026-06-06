/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <unistd.h>
#ifdef setresuid
#undef setresuid
#endif
int (*foo)(uid_t, uid_t, uid_t) = setresuid;
int main(void) { return 0; }
