#include <pthread.h>
#ifdef pthread_cond_destroy
#undef pthread_cond_destroy
#endif
int (*foo)(pthread_cond_t *) = pthread_cond_destroy;
int main(void) { return 0; }
