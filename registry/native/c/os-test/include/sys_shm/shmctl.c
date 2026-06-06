/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/shm.h>
#ifdef shmctl
#undef shmctl
#endif
int (*foo)(int, int, struct shmid_ds *) = shmctl;
int main(void) { return 0; }
