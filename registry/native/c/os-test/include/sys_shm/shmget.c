/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/shm.h>
#ifdef shmget
#undef shmget
#endif
int (*foo)(key_t, size_t, int) = shmget;
int main(void) { return 0; }
