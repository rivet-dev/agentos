#include <pthread.h>
#ifdef pthread_self
#undef pthread_self
#endif
pthread_t (*foo)(void) = pthread_self;
int main(void) { return 0; }
