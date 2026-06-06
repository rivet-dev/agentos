/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/sem.h>
#ifdef semget
#undef semget
#endif
int (*foo)(key_t, int, int) = semget;
int main(void) { return 0; }
