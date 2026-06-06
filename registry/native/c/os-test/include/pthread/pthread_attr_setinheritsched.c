/*[TPS]*/
#include <pthread.h>
#ifdef pthread_attr_setinheritsched
#undef pthread_attr_setinheritsched
#endif
int (*foo)(pthread_attr_t *, int) = pthread_attr_setinheritsched;
int main(void) { return 0; }
