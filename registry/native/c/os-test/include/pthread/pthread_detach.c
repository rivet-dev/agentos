#include <pthread.h>
#ifdef pthread_detach
#undef pthread_detach
#endif
int (*foo)(pthread_t) = pthread_detach;
int main(void) { return 0; }
