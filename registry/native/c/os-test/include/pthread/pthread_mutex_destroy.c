#include <pthread.h>
#ifdef pthread_mutex_destroy
#undef pthread_mutex_destroy
#endif
int (*foo)(pthread_mutex_t *) = pthread_mutex_destroy;
int main(void) { return 0; }
