/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/ipc.h>
#ifdef ftok
#undef ftok
#endif
key_t (*foo)(const char *, int) = ftok;
int main(void) { return 0; }
