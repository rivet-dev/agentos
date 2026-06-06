#include <pthread.h>
#ifdef pthread_join
#undef pthread_join
#endif
int (*foo)(pthread_t, void **) = pthread_join;
int main(void) { return 0; }
