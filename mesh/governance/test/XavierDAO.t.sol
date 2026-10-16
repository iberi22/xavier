// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "../contracts/XavierDAO.sol";

// Minimal cheatcode surface, so the suite runs without vendoring forge-std.
interface Vm {
    function prank(address msgSender) external;
    function warp(uint256 newTimestamp) external;
    function expectRevert(bytes calldata revertData) external;
}

contract XavierDAOTest {
    Vm internal constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    XavierDAO internal dao;

    address internal constant PROPOSER = address(0xA11CE);
    address internal constant COUNCIL_A = address(0xC01);
    address internal constant COUNCIL_B = address(0xC02);
    address internal constant VETO_GUARD = address(0xE7);
    address internal constant VOTER_1 = address(0x101);
    address internal constant VOTER_2 = address(0x102);
    address internal constant STRANGER = address(0xBAD);

    bytes32 internal constant CLUSTER = keccak256("cluster-1");

    // The test contract deploys the DAO, so it holds DEFAULT_ADMIN_ROLE.
    function setUp() public {
        dao = new XavierDAO();
        dao.grantRole(dao.PROPOSER_ROLE(), PROPOSER);
        dao.grantRole(dao.COUNCIL_ROLE(), COUNCIL_A);
        dao.grantRole(dao.COUNCIL_ROLE(), COUNCIL_B);
        dao.grantRole(dao.VETO_ROLE(), VETO_GUARD);
        dao.setVotingPower(VOTER_1, 80);
        dao.setVotingPower(VOTER_2, 40);
    }

    function _revertMissingRole() internal {
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Missing role"));
    }

    function _create() internal {
        vm.prank(PROPOSER);
        dao.createProposal(CLUSTER, "title", "description");
    }

    // Roles

    function test_deployerHoldsOnlyAdminRole() public view {
        require(dao.hasRole(dao.DEFAULT_ADMIN_ROLE(), address(this)), "deployer is not admin");
        require(!dao.hasRole(dao.PROPOSER_ROLE(), address(this)), "admin must not propose by default");
        require(!dao.hasRole(dao.COUNCIL_ROLE(), address(this)), "admin must not vote as council");
        require(!dao.hasRole(dao.VETO_ROLE(), address(this)), "admin must not veto by default");
    }

    // Role ids are read before vm.prank: the prank applies to the next external call.
    function test_onlyAdminCanGrantRoles() public {
        bytes32 role = dao.PROPOSER_ROLE();
        _revertMissingRole();
        vm.prank(STRANGER);
        dao.grantRole(role, STRANGER);
    }

    function test_onlyAdminCanRevokeRoles() public {
        bytes32 role = dao.PROPOSER_ROLE();
        _revertMissingRole();
        vm.prank(PROPOSER);
        dao.revokeRole(role, PROPOSER);
    }

    function test_revokedRoleLosesAccess() public {
        dao.revokeRole(dao.PROPOSER_ROLE(), PROPOSER);
        require(!dao.hasRole(dao.PROPOSER_ROLE(), PROPOSER), "role not revoked");
        _revertMissingRole();
        _create();
    }

    function test_holderCanRenounceOwnRole() public {
        bytes32 role = dao.VETO_ROLE();
        vm.prank(VETO_GUARD);
        dao.renounceRole(role);
        require(!dao.hasRole(dao.VETO_ROLE(), VETO_GUARD), "role not renounced");
    }

    function test_onlyAdminSetsVotingPower() public {
        _revertMissingRole();
        vm.prank(STRANGER);
        dao.setVotingPower(STRANGER, 1000);
    }

    // createProposal

    function test_proposerCreatesProposal() public {
        _create();
        (, , , , , bool vetoed, bool executed) = dao.getProposalStatus(CLUSTER);
        require(!vetoed && !executed, "new proposal must start open");
    }

    function test_strangerCannotCreateProposal() public {
        _revertMissingRole();
        vm.prank(STRANGER);
        dao.createProposal(CLUSTER, "title", "description");
    }

    function test_createRejectsZeroClusterId() public {
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Invalid cluster id"));
        vm.prank(PROPOSER);
        dao.createProposal(bytes32(0), "title", "description");
    }

    function test_createRejectsDuplicateCluster() public {
        _create();
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Proposal already exists"));
        vm.prank(PROPOSER);
        dao.createProposal(CLUSTER, "other", "other");
    }

    // castVote

    function test_councilFlagMustMatchRole() public {
        _create();
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Council flag mismatch"));
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 80, true);

        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Council flag mismatch"));
        vm.prank(COUNCIL_A);
        dao.castVote(CLUSTER, true, 0, false);
    }

    function test_votingPowerMustMatchRegistry() public {
        _create();
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Voting power mismatch"));
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 1000000, false);
    }

    function test_unregisteredAccountCannotVoteAsUser() public {
        _create();
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "No voting power"));
        vm.prank(STRANGER);
        dao.castVote(CLUSTER, true, 0, false);
    }

    function test_userVoteIsWeightedByRegistry() public {
        _create();
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 80, false);
        vm.prank(VOTER_2);
        dao.castVote(CLUSTER, false, 40, false);

        (, uint256 yes, uint256 no, , , , ) = dao.getProposalStatus(CLUSTER);
        require(yes == 80, "user yes weight");
        require(no == 40, "user no weight");
    }

    function test_cannotVoteTwice() public {
        _create();
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 80, false);
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Voter already voted"));
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 80, false);
    }

    function test_cannotVoteAfterVotingPeriod() public {
        _create();
        uint256 createdAt = block.timestamp;
        vm.warp(createdAt + 48 hours + 1);
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Voting period has ended"));
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 80, false);
    }

    // vetoProposal

    function test_strangerCannotVeto() public {
        _create();
        _revertMissingRole();
        vm.prank(STRANGER);
        dao.vetoProposal(CLUSTER, "reason");
    }

    function test_proposerCannotVeto() public {
        _create();
        _revertMissingRole();
        vm.prank(PROPOSER);
        dao.vetoProposal(CLUSTER, "reason");
    }

    function test_vetoRoleCanVeto() public {
        _create();
        vm.prank(VETO_GUARD);
        dao.vetoProposal(CLUSTER, "reason");
        (, , , , , bool vetoed, ) = dao.getProposalStatus(CLUSTER);
        require(vetoed, "proposal not vetoed");
    }

    // overruleVeto

    function test_nonVoterCannotOverrule() public {
        _create();
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 80, false);
        vm.prank(VETO_GUARD);
        dao.vetoProposal(CLUSTER, "reason");

        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Only voters can overrule"));
        vm.prank(STRANGER);
        dao.overruleVeto(CLUSTER);
    }

    function test_overruleNeedsSeventyFivePercent() public {
        _create();
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 80, false);
        vm.prank(VOTER_2);
        dao.castVote(CLUSTER, false, 40, false);
        vm.prank(VETO_GUARD);
        dao.vetoProposal(CLUSTER, "reason");

        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Overrule threshold not reached"));
        vm.prank(VOTER_2);
        dao.overruleVeto(CLUSTER);
    }

    function test_voterOverrulesVetoAtThreshold() public {
        _create();
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 80, false);
        vm.prank(VETO_GUARD);
        dao.vetoProposal(CLUSTER, "reason");

        vm.prank(VOTER_1);
        dao.overruleVeto(CLUSTER);
        (, , , , , bool vetoed, ) = dao.getProposalStatus(CLUSTER);
        require(!vetoed, "veto not overruled");
    }

    // executeProposal

    function test_executeApprovesWithQuorumAndTimelock() public {
        _create();
        uint256 createdAt = block.timestamp;
        vm.prank(COUNCIL_A);
        dao.castVote(CLUSTER, true, 0, true);
        vm.prank(COUNCIL_B);
        dao.castVote(CLUSTER, true, 0, true);
        vm.prank(VOTER_1);
        dao.castVote(CLUSTER, true, 80, false);
        vm.prank(VOTER_2);
        dao.castVote(CLUSTER, true, 40, false);

        vm.warp(createdAt + 48 hours);
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Timelock delay not met"));
        dao.executeProposal(CLUSTER);

        vm.warp(createdAt + 72 hours);
        dao.executeProposal(CLUSTER);
        (bool approved, , , , , , bool executed) = dao.getProposalStatus(CLUSTER);
        require(approved && executed, "proposal should be approved and executed");
    }

    function test_vetoedProposalCannotExecute() public {
        _create();
        uint256 createdAt = block.timestamp;
        vm.prank(VETO_GUARD);
        dao.vetoProposal(CLUSTER, "reason");

        vm.warp(createdAt + 72 hours);
        vm.expectRevert(abi.encodeWithSignature("Error(string)", "Proposal is vetoed"));
        dao.executeProposal(CLUSTER);
    }
}
