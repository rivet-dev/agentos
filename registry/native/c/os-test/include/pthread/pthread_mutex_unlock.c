#include <pthread.h>
#ifdef pthread_mutex_unlock
#undef pthread_mutex_unlock
#endif
int (*foo)(pthread_mutex_t *) = pthread_mutex_unlock;
int main(void) { return 0; }
