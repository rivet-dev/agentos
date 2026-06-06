#include <pthread.h>
#ifdef pthread_mutex_consistent
#undef pthread_mutex_consistent
#endif
int (*foo)(pthread_mutex_t *) = pthread_mutex_consistent;
int main(void) { return 0; }
