#include <pthread.h>
#ifdef pthread_exit
#undef pthread_exit
#endif
 void (*foo)(void *) = pthread_exit;
int main(void) { return 0; }
