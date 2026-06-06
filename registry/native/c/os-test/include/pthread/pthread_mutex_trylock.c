#include <pthread.h>
#ifdef pthread_mutex_trylock
#undef pthread_mutex_trylock
#endif
int (*foo)(pthread_mutex_t *) = pthread_mutex_trylock;
int main(void) { return 0; }
