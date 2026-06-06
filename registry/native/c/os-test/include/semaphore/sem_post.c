#include <semaphore.h>
#ifdef sem_post
#undef sem_post
#endif
int (*foo)(sem_t *) = sem_post;
int main(void) { return 0; }
