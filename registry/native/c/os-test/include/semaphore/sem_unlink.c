#include <semaphore.h>
#ifdef sem_unlink
#undef sem_unlink
#endif
int (*foo)(const char *) = sem_unlink;
int main(void) { return 0; }
