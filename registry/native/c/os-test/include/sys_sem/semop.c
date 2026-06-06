/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/sem.h>
#ifdef semop
#undef semop
#endif
int (*foo)(int, struct sembuf *, size_t) = semop;
int main(void) { return 0; }
