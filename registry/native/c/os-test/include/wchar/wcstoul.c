#include <wchar.h>
#ifdef wcstoul
#undef wcstoul
#endif
unsigned long (*foo)(const wchar_t *restrict, wchar_t **restrict, int) = wcstoul;
int main(void) { return 0; }
