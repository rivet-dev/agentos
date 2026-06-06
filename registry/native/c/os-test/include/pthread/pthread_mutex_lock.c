#include <pthread.h>
#ifdef pthread_mutex_lock
#undef pthread_mutex_lock
#endif
int (*foo)(pthread_mutex_t *) = pthread_mutex_lock;
int main(void) { return 0; }
