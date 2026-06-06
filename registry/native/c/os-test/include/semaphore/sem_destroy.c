#include <semaphore.h>
#ifdef sem_destroy
#undef sem_destroy
#endif
int (*foo)(sem_t *) = sem_destroy;
int main(void) { return 0; }
