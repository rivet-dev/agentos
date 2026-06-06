/*[TSH]*/
#include <pthread.h>
#ifdef pthread_condattr_setpshared
#undef pthread_condattr_setpshared
#endif
int (*foo)(pthread_condattr_t *, int) = pthread_condattr_setpshared;
int main(void) { return 0; }
