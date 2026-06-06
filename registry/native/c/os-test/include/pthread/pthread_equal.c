#include <pthread.h>
#ifdef pthread_equal
#undef pthread_equal
#endif
int (*foo)(pthread_t, pthread_t) = pthread_equal;
int main(void) { return 0; }
