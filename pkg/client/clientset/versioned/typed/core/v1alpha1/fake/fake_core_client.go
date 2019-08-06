/* Copyright Microsoft */

// Code generated by client-gen. DO NOT EDIT.

package fake

import (
	v1alpha1 "github.com/microsoft/scylla/pkg/client/clientset/versioned/typed/core/v1alpha1"
	rest "k8s.io/client-go/rest"
	testing "k8s.io/client-go/testing"
)

type FakeCoreV1alpha1 struct {
	*testing.Fake
}

func (c *FakeCoreV1alpha1) OperationalConfigurations(namespace string) v1alpha1.OperationalConfigurationInterface {
	return &FakeOperationalConfigurations{c, namespace}
}

// RESTClient returns a RESTClient that is used to communicate
// with API server by this client implementation.
func (c *FakeCoreV1alpha1) RESTClient() rest.Interface {
	var ret *rest.RESTClient
	return ret
}
