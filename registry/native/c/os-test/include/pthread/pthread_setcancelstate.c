#include <pthread.h>
#ifdef pthread_setcancelstate
#undef pthread_setcancelstate
#endif
int (*foo)(int, int *) = pthread_setcancelstate;
int main(void) { return 0; }
