#include <pthread.h>
#ifdef pthread_setcanceltype
#undef pthread_setcanceltype
#endif
int (*foo)(int, int *) = pthread_setcanceltype;
int main(void) { return 0; }
