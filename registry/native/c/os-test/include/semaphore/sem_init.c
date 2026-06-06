#include <semaphore.h>
#ifdef sem_init
#undef sem_init
#endif
int (*foo)(sem_t *, int, unsigned) = sem_init;
int main(void) { return 0; }
